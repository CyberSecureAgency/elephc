//! Purpose:
//! Lowers PHP `array_key_exists()` calls for indexed arrays and associative hashes
//! in the Phase 04 EIR backend.
//!
//! Called from:
//! - `crate::codegen_ir::lower_inst::builtins::arrays::lower_array_key_exists()`.
//!
//! Key details:
//! - Indexed arrays use `__rt_array_key_exists` with integer-like keys.
//! - Associative arrays probe `__rt_hash_get`; its found flag is already a PHP bool result.

use crate::codegen::abi;
use crate::codegen::platform::Arch;
use crate::codegen_ir::context::FunctionContext;
use crate::codegen_ir::{CodegenIrError, Result};
use crate::ir::{Instruction, ValueId};
use crate::types::PhpType;

use super::super::super::{expect_operand, store_if_result};

/// Lowers `array_key_exists()` for indexed arrays and associative arrays.
pub(super) fn lower_array_key_exists(ctx: &mut FunctionContext<'_>, inst: &Instruction) -> Result<()> {
    super::super::ensure_arg_count(inst, "array_key_exists", 2)?;
    let key = expect_operand(inst, 0)?;
    let array = expect_operand(inst, 1)?;
    match ctx.value_php_type(array)?.codegen_repr() {
        PhpType::Array(_) => lower_indexed_array_key_exists(ctx, inst, key, array),
        PhpType::AssocArray { .. } => lower_assoc_array_key_exists(ctx, inst, key, array),
        other => Err(CodegenIrError::unsupported(format!(
            "array_key_exists for PHP array type {:?}",
            other
        ))),
    }
}

/// Lowers indexed-array key existence through the bounds-check runtime helper.
fn lower_indexed_array_key_exists(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
    key: ValueId,
    array: ValueId,
) -> Result<()> {
    require_indexed_key_type(ctx.value_php_type(key)?)?;
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.load_value_to_reg(array, "x0")?;
            ctx.load_value_to_reg(key, "x1")?;
        }
        Arch::X86_64 => {
            ctx.load_value_to_reg(array, "rdi")?;
            ctx.load_value_to_reg(key, "rsi")?;
        }
    }
    abi::emit_call_label(ctx.emitter, "__rt_array_key_exists");
    store_if_result(ctx, inst)
}

/// Lowers associative-array key existence by probing the hash table.
fn lower_assoc_array_key_exists(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
    key: ValueId,
    array: ValueId,
) -> Result<()> {
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.load_value_to_reg(array, "x0")?;
            materialize_hash_key_aarch64(ctx, key)?;
            abi::emit_call_label(ctx.emitter, "__rt_hash_get");
        }
        Arch::X86_64 => {
            ctx.load_value_to_reg(array, "rdi")?;
            materialize_hash_key_x86_64(ctx, key)?;
            abi::emit_call_label(ctx.emitter, "__rt_hash_get");
        }
    }
    store_if_result(ctx, inst)
}

/// Materializes an EIR value as a normalized AArch64 associative-array key.
fn materialize_hash_key_aarch64(ctx: &mut FunctionContext<'_>, key: ValueId) -> Result<()> {
    match ctx.value_php_type(key)?.codegen_repr() {
        PhpType::Str => ctx.load_string_value_to_regs(key, "x1", "x2"),
        PhpType::Int | PhpType::Bool | PhpType::Callable => {
            ctx.load_value_to_reg(key, "x1")?;
            abi::emit_load_int_immediate(ctx.emitter, "x2", -1);
            Ok(())
        }
        other => Err(CodegenIrError::unsupported(format!(
            "array_key_exists key PHP type {:?}",
            other
        ))),
    }
}

/// Materializes an EIR value as a normalized x86_64 associative-array key.
fn materialize_hash_key_x86_64(ctx: &mut FunctionContext<'_>, key: ValueId) -> Result<()> {
    match ctx.value_php_type(key)?.codegen_repr() {
        PhpType::Str => ctx.load_string_value_to_regs(key, "rsi", "rdx"),
        PhpType::Int | PhpType::Bool | PhpType::Callable => {
            ctx.load_value_to_reg(key, "rsi")?;
            abi::emit_load_int_immediate(ctx.emitter, "rdx", -1);
            Ok(())
        }
        other => Err(CodegenIrError::unsupported(format!(
            "array_key_exists key PHP type {:?}",
            other
        ))),
    }
}

/// Verifies indexed-array key existence can use the integer-key runtime helper.
fn require_indexed_key_type(key_ty: PhpType) -> Result<()> {
    match key_ty.codegen_repr() {
        PhpType::Int | PhpType::Bool => Ok(()),
        other => Err(CodegenIrError::unsupported(format!(
            "array_key_exists key PHP type {:?}",
            other
        ))),
    }
}
