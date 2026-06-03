//! Purpose:
//! Lowers string constants, scalar-to-string conversions, and string
//! concatenation EIR opcodes for the Phase 04 backend.
//!
//! Called from:
//! - `crate::codegen_ir::lower_inst::lower_instruction()`.
//!
//! Key details:
//! - PHP string coercion treats `false` and `null` as empty strings, while
//!   integer true and ordinary ints use the existing `__rt_itoa` helper.

use crate::codegen::abi;
use crate::codegen::platform::Arch;
use crate::ir::Instruction;
use crate::types::PhpType;

use super::super::context::FunctionContext;
use super::{expect_data, expect_operand, require_float, require_string, store_if_result};
use crate::codegen_ir::{CodegenIrError, Result};

/// Lowers a string constant by materializing its data-section pointer and byte length.
pub(super) fn lower_const_str(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    let data_id = expect_data(inst)?;
    let (label, len) = ctx.intern_string_data(data_id)?;
    let (ptr_reg, len_reg) = abi::string_result_regs(ctx.emitter);
    abi::emit_symbol_address(ctx.emitter, ptr_reg, &label);
    abi::emit_load_int_immediate(ctx.emitter, len_reg, len as i64);
    store_if_result(ctx, inst)
}

/// Lowers a string concatenation by loading both string pairs into `__rt_concat`'s ABI.
pub(super) fn lower_str_concat(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    let lhs = expect_operand(inst, 0)?;
    let rhs = expect_operand(inst, 1)?;
    require_string(ctx.value_php_type(lhs)?, inst)?;
    require_string(ctx.value_php_type(rhs)?, inst)?;
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.load_string_value_to_regs(lhs, "x1", "x2")?;
            ctx.load_string_value_to_regs(rhs, "x3", "x4")?;
        }
        Arch::X86_64 => {
            ctx.load_string_value_to_regs(lhs, "rax", "rdx")?;
            ctx.load_string_value_to_regs(rhs, "rdi", "rsi")?;
        }
    }
    abi::emit_call_label(ctx.emitter, "__rt_concat");
    store_if_result(ctx, inst)
}

/// Lowers a float-to-string conversion through the existing runtime formatter.
pub(super) fn lower_float_to_string(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
) -> Result<()> {
    let value = expect_operand(inst, 0)?;
    require_float(ctx.load_value_to_result(value)?, inst)?;
    abi::emit_call_label(ctx.emitter, "__rt_ftoa");
    store_if_result(ctx, inst)
}

/// Lowers an integer-like-to-string conversion, including PHP bool/null string rules.
pub(super) fn lower_int_like_to_string(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
) -> Result<()> {
    let value = expect_operand(inst, 0)?;
    match ctx.load_value_to_result(value)? {
        PhpType::Bool => {
            lower_loaded_bool_to_string(ctx)?;
            store_if_result(ctx, inst)
        }
        PhpType::Int => {
            abi::emit_call_label(ctx.emitter, "__rt_itoa");
            store_if_result(ctx, inst)
        }
        PhpType::Void | PhpType::Never => {
            let len_reg = abi::string_result_regs(ctx.emitter).1;
            abi::emit_load_int_immediate(ctx.emitter, len_reg, 0);
            store_if_result(ctx, inst)
        }
        other => Err(CodegenIrError::unsupported(format!(
            "{} for PHP type {:?}",
            inst.op.name(),
            other
        ))),
    }
}

/// Converts the loaded boolean result to PHP string ABI registers.
fn lower_loaded_bool_to_string(ctx: &mut FunctionContext<'_>) -> Result<()> {
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            let false_label = ctx.next_label("bool_to_str_false");
            let done_label = ctx.next_label("bool_to_str_done");
            ctx.emitter.instruction(&format!("cbz x0, {}", false_label));       // false stringifies to an empty string
            abi::emit_call_label(ctx.emitter, "__rt_itoa");
            ctx.emitter.instruction(&format!("b {}", done_label));              // skip the empty-string fallback after true conversion
            ctx.emitter.label(&false_label);
            ctx.emitter.instruction("mov x2, #0");                              // false has zero string length
            ctx.emitter.label(&done_label);
        }
        Arch::X86_64 => {
            let false_label = ctx.next_label("bool_to_str_false");
            let done_label = ctx.next_label("bool_to_str_done");
            ctx.emitter.instruction("test rax, rax");                           // test whether the boolean payload is false
            ctx.emitter.instruction(&format!("je {}", false_label));            // false stringifies to an empty string
            abi::emit_call_label(ctx.emitter, "__rt_itoa");
            ctx.emitter.instruction(&format!("jmp {}", done_label));            // skip the empty-string fallback after true conversion
            ctx.emitter.label(&false_label);
            ctx.emitter.instruction("mov rdx, 0");                              // false has zero string length
            ctx.emitter.label(&done_label);
        }
    }
    Ok(())
}
