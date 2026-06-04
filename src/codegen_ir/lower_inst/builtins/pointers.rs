//! Purpose:
//! Lowers compiler-extension pointer builtins for the EIR backend.
//! Covers raw null materialization, null tests, and byte-offset address arithmetic.
//!
//! Called from:
//! - `crate::codegen_ir::lower_inst::builtins::lower_builtin_call()`.
//!
//! Key details:
//! - Pointer values are raw machine addresses in the integer result register.
//! - These builtins do not allocate, box, retain, or release PHP runtime values.

use crate::codegen::abi;
use crate::codegen::platform::Arch;
use crate::codegen_ir::{CodegenIrError, Result};
use crate::ir::Instruction;
use crate::types::PhpType;

use super::super::super::context::FunctionContext;
use super::{expect_operand, store_if_result};

/// Lowers `ptr_null()` by materializing the raw null pointer sentinel.
pub(super) fn lower_ptr_null(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    ensure_arg_count(inst, "ptr_null", 0)?;
    abi::emit_load_int_immediate(ctx.emitter, abi::int_result_reg(ctx.emitter), 0);
    store_if_result(ctx, inst)
}

/// Lowers `ptr_is_null(pointer)` by comparing the raw pointer address to zero.
pub(super) fn lower_ptr_is_null(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    ensure_arg_count(inst, "ptr_is_null", 1)?;
    let pointer = expect_operand(inst, 0)?;
    require_pointer(ctx.load_value_to_result(pointer)?, "ptr_is_null")?;
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction("cmp x0, #0");                              // compare the raw pointer payload against the null address
            ctx.emitter.instruction("cset x0, eq");                             // return true only when the pointer payload is null
        }
        Arch::X86_64 => {
            ctx.emitter.instruction("test rax, rax");                           // compare the raw pointer payload against the null address
            ctx.emitter.instruction("sete al");                                 // materialize the null test result in the low byte
            ctx.emitter.instruction("movzx rax, al");                           // widen the null test result to the integer result register
        }
    }
    store_if_result(ctx, inst)
}

/// Lowers `ptr_offset(pointer, offset)` by adding a byte offset to a raw address.
pub(super) fn lower_ptr_offset(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    ensure_arg_count(inst, "ptr_offset", 2)?;
    let pointer = expect_operand(inst, 0)?;
    let offset = expect_operand(inst, 1)?;
    require_pointer(ctx.load_value_to_result(pointer)?, "ptr_offset")?;
    abi::emit_push_reg(ctx.emitter, abi::int_result_reg(ctx.emitter));
    require_integer_offset(ctx.load_value_to_result(offset)?, "ptr_offset")?;
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction("mov x10, x0");                             // preserve the byte offset while restoring the base pointer
            abi::emit_pop_reg(ctx.emitter, "x0");
            ctx.emitter.instruction("add x0, x0, x10");                         // compute the derived raw pointer address
        }
        Arch::X86_64 => {
            ctx.emitter.instruction("mov r10, rax");                            // preserve the byte offset while restoring the base pointer
            abi::emit_pop_reg(ctx.emitter, "rax");
            ctx.emitter.instruction("add rax, r10");                            // compute the derived raw pointer address
        }
    }
    store_if_result(ctx, inst)
}

/// Lowers `ptr_get(pointer)` by reading one machine word through a checked pointer.
pub(super) fn lower_ptr_get(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    lower_pointer_read(ctx, inst, "ptr_get", PointerWidth::Word64)
}

/// Lowers `ptr_set(pointer, value)` by writing one machine word through a checked pointer.
pub(super) fn lower_ptr_set(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    lower_pointer_write(ctx, inst, "ptr_set", PointerWidth::Word64, WordValuePolicy::Word)
}

/// Lowers `ptr_read8(pointer)` by reading one unsigned byte through a checked pointer.
pub(super) fn lower_ptr_read8(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    lower_pointer_read(ctx, inst, "ptr_read8", PointerWidth::Byte)
}

/// Lowers `ptr_read16(pointer)` by reading one unsigned 16-bit word through a checked pointer.
pub(super) fn lower_ptr_read16(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    lower_pointer_read(ctx, inst, "ptr_read16", PointerWidth::Half)
}

/// Lowers `ptr_read32(pointer)` by reading one unsigned 32-bit word through a checked pointer.
pub(super) fn lower_ptr_read32(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    lower_pointer_read(ctx, inst, "ptr_read32", PointerWidth::Word32)
}

/// Lowers `ptr_write8(pointer, value)` by writing one byte through a checked pointer.
pub(super) fn lower_ptr_write8(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    lower_pointer_write(ctx, inst, "ptr_write8", PointerWidth::Byte, WordValuePolicy::IntOnly)
}

/// Lowers `ptr_write16(pointer, value)` by writing one 16-bit word through a checked pointer.
pub(super) fn lower_ptr_write16(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    lower_pointer_write(ctx, inst, "ptr_write16", PointerWidth::Half, WordValuePolicy::IntOnly)
}

/// Lowers `ptr_write32(pointer, value)` by writing one 32-bit word through a checked pointer.
pub(super) fn lower_ptr_write32(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    lower_pointer_write(ctx, inst, "ptr_write32", PointerWidth::Word32, WordValuePolicy::IntOnly)
}

/// Native integer width for raw pointer memory reads and writes.
#[derive(Clone, Copy)]
enum PointerWidth {
    Byte,
    Half,
    Word32,
    Word64,
}

/// Controls which PHP value types a pointer write builtin can materialize as a raw word.
#[derive(Clone, Copy)]
enum WordValuePolicy {
    IntOnly,
    Word,
}

/// Lowers a raw pointer memory read after validating the pointer against null.
fn lower_pointer_read(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
    name: &str,
    width: PointerWidth,
) -> Result<()> {
    ensure_arg_count(inst, name, 1)?;
    let pointer = expect_operand(inst, 0)?;
    load_checked_pointer(ctx, pointer, name)?;
    emit_width_load(ctx, width);
    store_if_result(ctx, inst)
}

/// Lowers a raw pointer memory write after validating the destination pointer against null.
fn lower_pointer_write(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
    name: &str,
    width: PointerWidth,
    policy: WordValuePolicy,
) -> Result<()> {
    ensure_arg_count(inst, name, 2)?;
    let pointer = expect_operand(inst, 0)?;
    let value = expect_operand(inst, 1)?;
    load_checked_pointer(ctx, pointer, name)?;
    abi::emit_push_reg(ctx.emitter, abi::int_result_reg(ctx.emitter));
    materialize_word_value(ctx, value, name, policy)?;
    emit_width_store(ctx, width);
    emit_void_result(ctx);
    store_if_result(ctx, inst)
}

/// Loads a pointer operand, validates its type, and aborts at runtime if it is null.
fn load_checked_pointer(
    ctx: &mut FunctionContext<'_>,
    value: crate::ir::ValueId,
    name: &str,
) -> Result<()> {
    require_pointer(ctx.load_value_to_result(value)?, name)?;
    abi::emit_call_label(ctx.emitter, "__rt_ptr_check_nonnull");
    Ok(())
}

/// Materializes an EIR value as the raw word payload for a pointer store.
fn materialize_word_value(
    ctx: &mut FunctionContext<'_>,
    value: crate::ir::ValueId,
    name: &str,
    policy: WordValuePolicy,
) -> Result<()> {
    match ctx.value_php_type(value)?.codegen_repr() {
        PhpType::Int | PhpType::Bool => {
            ctx.load_value_to_result(value)?;
            Ok(())
        }
        PhpType::Void | PhpType::Never if matches!(policy, WordValuePolicy::Word) => {
            abi::emit_load_int_immediate(ctx.emitter, abi::int_result_reg(ctx.emitter), 0);
            Ok(())
        }
        PhpType::Pointer(_) if matches!(policy, WordValuePolicy::Word) => {
            ctx.load_value_to_result(value)?;
            Ok(())
        }
        other => Err(CodegenIrError::unsupported(format!(
            "{} value PHP type {:?}",
            name,
            other
        ))),
    }
}

/// Emits the target-specific load for a raw pointer memory width.
fn emit_width_load(ctx: &mut FunctionContext<'_>, width: PointerWidth) {
    match (ctx.emitter.target.arch, width) {
        (Arch::AArch64, PointerWidth::Byte) => {
            ctx.emitter.instruction("ldrb w0, [x0]");                           // load one unsigned byte and zero-extend it as a PHP integer
        }
        (Arch::AArch64, PointerWidth::Half) => {
            ctx.emitter.instruction("ldrh w0, [x0]");                           // load one unsigned 16-bit word and zero-extend it as a PHP integer
        }
        (Arch::AArch64, PointerWidth::Word32) => {
            ctx.emitter.instruction("ldr w0, [x0]");                            // load one unsigned 32-bit word and zero-extend it as a PHP integer
        }
        (Arch::AArch64, PointerWidth::Word64) => {
            ctx.emitter.instruction("ldr x0, [x0]");                            // load one machine word as a PHP integer
        }
        (Arch::X86_64, PointerWidth::Byte) => {
            ctx.emitter.instruction("movzx eax, BYTE PTR [rax]");               // load one unsigned byte and zero-extend it as a PHP integer
        }
        (Arch::X86_64, PointerWidth::Half) => {
            ctx.emitter.instruction("movzx eax, WORD PTR [rax]");               // load one unsigned 16-bit word and zero-extend it as a PHP integer
        }
        (Arch::X86_64, PointerWidth::Word32) => {
            ctx.emitter.instruction("mov eax, DWORD PTR [rax]");                // load one unsigned 32-bit word and zero-extend it as a PHP integer
        }
        (Arch::X86_64, PointerWidth::Word64) => {
            ctx.emitter.instruction("mov rax, QWORD PTR [rax]");                // load one machine word as a PHP integer
        }
    }
}

/// Emits the target-specific store for a raw pointer memory width.
fn emit_width_store(ctx: &mut FunctionContext<'_>, width: PointerWidth) {
    match (ctx.emitter.target.arch, width) {
        (Arch::AArch64, PointerWidth::Byte) => {
            ctx.emitter.instruction("mov w10, w0");                             // preserve the low byte payload while restoring the pointer
            abi::emit_pop_reg(ctx.emitter, "x0");
            ctx.emitter.instruction("strb w10, [x0]");                          // store one byte through the checked pointer
        }
        (Arch::AArch64, PointerWidth::Half) => {
            ctx.emitter.instruction("mov w10, w0");                             // preserve the low 16-bit payload while restoring the pointer
            abi::emit_pop_reg(ctx.emitter, "x0");
            ctx.emitter.instruction("strh w10, [x0]");                          // store one 16-bit word through the checked pointer
        }
        (Arch::AArch64, PointerWidth::Word32) => {
            ctx.emitter.instruction("mov w10, w0");                             // preserve the low 32-bit payload while restoring the pointer
            abi::emit_pop_reg(ctx.emitter, "x0");
            ctx.emitter.instruction("str w10, [x0]");                           // store one 32-bit word through the checked pointer
        }
        (Arch::AArch64, PointerWidth::Word64) => {
            ctx.emitter.instruction("mov x10, x0");                             // preserve the machine-word payload while restoring the pointer
            abi::emit_pop_reg(ctx.emitter, "x0");
            ctx.emitter.instruction("str x10, [x0]");                           // store one machine word through the checked pointer
        }
        (Arch::X86_64, PointerWidth::Byte) => {
            ctx.emitter.instruction("mov cl, al");                              // preserve the low byte payload while restoring the pointer
            abi::emit_pop_reg(ctx.emitter, "rax");
            ctx.emitter.instruction("mov BYTE PTR [rax], cl");                  // store one byte through the checked pointer
        }
        (Arch::X86_64, PointerWidth::Half) => {
            ctx.emitter.instruction("mov cx, ax");                              // preserve the low 16-bit payload while restoring the pointer
            abi::emit_pop_reg(ctx.emitter, "rax");
            ctx.emitter.instruction("mov WORD PTR [rax], cx");                  // store one 16-bit word through the checked pointer
        }
        (Arch::X86_64, PointerWidth::Word32) => {
            ctx.emitter.instruction("mov ecx, eax");                            // preserve the low 32-bit payload while restoring the pointer
            abi::emit_pop_reg(ctx.emitter, "rax");
            ctx.emitter.instruction("mov DWORD PTR [rax], ecx");                // store one 32-bit word through the checked pointer
        }
        (Arch::X86_64, PointerWidth::Word64) => {
            ctx.emitter.instruction("mov rcx, rax");                            // preserve the machine-word payload while restoring the pointer
            abi::emit_pop_reg(ctx.emitter, "rax");
            ctx.emitter.instruction("mov QWORD PTR [rax], rcx");                // store one machine word through the checked pointer
        }
    }
}

/// Materializes the EIR void/null sentinel for storing a void write result.
fn emit_void_result(ctx: &mut FunctionContext<'_>) {
    abi::emit_load_int_immediate(
        ctx.emitter,
        abi::int_result_reg(ctx.emitter),
        0x7fff_ffff_ffff_fffe,
    );
}

/// Verifies a pointer builtin received the expected number of operands.
fn ensure_arg_count(inst: &Instruction, name: &str, expected: usize) -> Result<()> {
    if inst.operands.len() == expected {
        return Ok(());
    }
    Err(CodegenIrError::invalid_module(format!(
        "{} expected {} args, got {}",
        name,
        expected,
        inst.operands.len()
    )))
}

/// Verifies a pointer builtin operand has a pointer representation.
fn require_pointer(ty: PhpType, name: &str) -> Result<()> {
    match ty.codegen_repr() {
        PhpType::Pointer(_) => Ok(()),
        other => Err(CodegenIrError::unsupported(format!(
            "{} for pointer PHP type {:?}",
            name,
            other
        ))),
    }
}

/// Verifies `ptr_offset()` received an integer-like byte offset operand.
fn require_integer_offset(ty: PhpType, name: &str) -> Result<()> {
    match ty.codegen_repr() {
        PhpType::Int | PhpType::Bool => Ok(()),
        other => Err(CodegenIrError::unsupported(format!(
            "{} offset PHP type {:?}",
            name,
            other
        ))),
    }
}
