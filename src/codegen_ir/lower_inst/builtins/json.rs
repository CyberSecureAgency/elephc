//! Purpose:
//! Lowers JSON state and validation builtins for the EIR backend.
//! Bridges already-evaluated EIR operands to the shared JSON runtime helpers.
//!
//! Called from:
//! - `crate::codegen_ir::lower_inst::builtins::lower_builtin_call()`.
//!
//! Key details:
//! - JSON error state is runtime-global and must be reset after PHP arguments
//!   have already been evaluated by preceding EIR instructions.

use crate::codegen::abi;
use crate::codegen::platform::Arch;
use crate::codegen_ir::{CodegenIrError, Result};
use crate::ir::{Instruction, ValueId};
use crate::types::PhpType;

use super::super::super::context::FunctionContext;
use super::super::load_value_to_first_int_arg;
use super::{expect_operand, store_if_result};

/// Lowers `json_last_error()` by reading the shared runtime error-code symbol.
pub(super) fn lower_json_last_error(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
) -> Result<()> {
    super::ensure_arg_count(inst, "json_last_error", 0)?;
    abi::emit_load_symbol_to_reg(
        ctx.emitter,
        abi::int_result_reg(ctx.emitter),
        "_json_last_error",
        0,
    );
    store_if_result(ctx, inst)
}

/// Lowers `json_last_error_msg()` through the runtime message lookup table.
pub(super) fn lower_json_last_error_msg(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
) -> Result<()> {
    super::ensure_arg_count(inst, "json_last_error_msg", 0)?;
    abi::emit_call_label(ctx.emitter, "__rt_json_last_error_msg");
    store_if_result(ctx, inst)
}

/// Lowers `json_validate(json, depth?, flags?)` into the shared validator runtime.
pub(super) fn lower_json_validate(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
) -> Result<()> {
    ensure_arg_count_between(inst, "json_validate", 1, 3)?;
    let json = expect_operand(inst, 0)?;

    reset_json_validation_state(ctx);
    lower_json_validate_flags(ctx, inst)?;
    lower_json_validate_depth(ctx, inst)?;
    load_json_source_for_validate(ctx, json)?;
    abi::emit_call_label(ctx.emitter, "__rt_json_validate");
    store_if_result(ctx, inst)
}

/// Clears observable JSON error and parser state after all EIR operands have evaluated.
fn reset_json_validation_state(ctx: &mut FunctionContext<'_>) {
    abi::emit_store_zero_to_symbol(ctx.emitter, "_json_last_error", 0);
    abi::emit_store_zero_to_symbol(ctx.emitter, "_json_active_depth", 0);
    abi::emit_store_zero_to_symbol(ctx.emitter, "_json_error_location_active", 0);
    abi::emit_store_zero_to_symbol(ctx.emitter, "_json_error_source_ptr", 0);
}

/// Stores the active `json_validate()` flags, keeping only PHP's accepted bit.
fn lower_json_validate_flags(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
) -> Result<()> {
    if inst.operands.len() < 3 {
        abi::emit_store_zero_to_symbol(ctx.emitter, "_json_active_flags", 0);
        return Ok(());
    }
    let flags = expect_operand(inst, 2)?;
    require_integer_like(ctx.load_value_to_result(flags)?, "json_validate flags")?;
    mask_json_validate_flags(ctx);
    abi::emit_store_reg_to_symbol(
        ctx.emitter,
        abi::int_result_reg(ctx.emitter),
        "_json_active_flags",
        0,
    );
    Ok(())
}

/// Stores the strict depth limit used by the shared JSON validator.
fn lower_json_validate_depth(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
) -> Result<()> {
    if inst.operands.len() >= 2 {
        let depth = expect_operand(inst, 1)?;
        require_integer_like(ctx.load_value_to_result(depth)?, "json_validate depth")?;
        subtract_one_from_int_result(ctx);
        abi::emit_store_reg_to_symbol(
            ctx.emitter,
            abi::int_result_reg(ctx.emitter),
            "_json_depth_limit",
            0,
        );
    } else {
        abi::emit_load_int_immediate(ctx.emitter, abi::int_result_reg(ctx.emitter), 511);
        abi::emit_store_reg_to_symbol(
            ctx.emitter,
            abi::int_result_reg(ctx.emitter),
            "_json_depth_limit",
            0,
        );
    }
    Ok(())
}

/// Loads the JSON source string into the runtime helper's expected result registers.
fn load_json_source_for_validate(
    ctx: &mut FunctionContext<'_>,
    value: ValueId,
) -> Result<()> {
    match ctx.value_php_type(value)? {
        PhpType::Str => {
            let (ptr_reg, len_reg) = abi::string_result_regs(ctx.emitter);
            ctx.load_string_value_to_regs(value, ptr_reg, len_reg)
        }
        PhpType::Int => {
            ctx.load_value_to_result(value)?;
            abi::emit_call_label(ctx.emitter, "__rt_itoa");
            Ok(())
        }
        PhpType::Float => {
            ctx.load_value_to_result(value)?;
            abi::emit_call_label(ctx.emitter, "__rt_ftoa");
            Ok(())
        }
        PhpType::Bool => lower_bool_json_source(ctx, value),
        PhpType::Void | PhpType::Never => {
            emit_static_string_result(ctx, b"");
            Ok(())
        }
        PhpType::Mixed | PhpType::Union(_) => {
            load_value_to_first_int_arg(ctx, value)?;
            abi::emit_call_label(ctx.emitter, "__rt_mixed_cast_string");
            Ok(())
        }
        other => Err(CodegenIrError::unsupported(format!(
            "json_validate source for PHP type {:?}",
            other
        ))),
    }
}

/// Coerces a dynamic boolean JSON source to PHP's string form.
fn lower_bool_json_source(ctx: &mut FunctionContext<'_>, value: ValueId) -> Result<()> {
    let true_label = ctx.next_label("json_validate_bool_true");
    let done_label = ctx.next_label("json_validate_bool_done");
    ctx.load_value_to_result(value)?;
    abi::emit_branch_if_int_result_nonzero(ctx.emitter, &true_label);
    emit_static_string_result(ctx, b"");
    abi::emit_jump(ctx.emitter, &done_label);
    ctx.emitter.label(&true_label);
    emit_static_string_result(ctx, b"1");
    ctx.emitter.label(&done_label);
    Ok(())
}

/// Materializes a static string result pair for scalar JSON source coercions.
fn emit_static_string_result(ctx: &mut FunctionContext<'_>, bytes: &[u8]) {
    let (label, len) = ctx.data.add_string(bytes);
    let (ptr_reg, len_reg) = abi::string_result_regs(ctx.emitter);
    abi::emit_symbol_address(ctx.emitter, ptr_reg, &label);
    abi::emit_load_int_immediate(ctx.emitter, len_reg, len as i64);
}

/// Masks unsupported validate flags, preserving only `JSON_INVALID_UTF8_IGNORE`.
fn mask_json_validate_flags(ctx: &mut FunctionContext<'_>) {
    let reg = abi::int_result_reg(ctx.emitter);
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction("mov x9, #1048576");                        // mask = JSON_INVALID_UTF8_IGNORE, the only json_validate flag PHP allows
            ctx.emitter.instruction(&format!("and {reg}, {reg}, x9"));          // ignore dynamically supplied unsupported validate flags
        }
        Arch::X86_64 => {
            ctx.emitter.instruction(&format!("and {reg}, 1048576"));            // keep only JSON_INVALID_UTF8_IGNORE for dynamic validate flags
        }
    }
}

/// Applies the strict-depth `depth - 1` runtime convention in the integer result register.
fn subtract_one_from_int_result(ctx: &mut FunctionContext<'_>) {
    let reg = abi::int_result_reg(ctx.emitter);
    match ctx.emitter.target.arch {
        Arch::AArch64 => {
            ctx.emitter.instruction(&format!("sub {reg}, {reg}, #1"));          // convert PHP json_validate depth to the runtime strict-depth limit
        }
        Arch::X86_64 => {
            ctx.emitter.instruction(&format!("sub {reg}, 1"));                  // convert PHP json_validate depth to the runtime strict-depth limit
        }
    }
}

/// Verifies a value can be passed as a JSON integer option.
fn require_integer_like(ty: PhpType, context: &str) -> Result<()> {
    if matches!(ty, PhpType::Int | PhpType::Bool) {
        return Ok(());
    }
    Err(CodegenIrError::unsupported(format!(
        "{} for PHP type {:?}",
        context,
        ty
    )))
}

/// Verifies that the builtin call has between the expected lowered operand counts.
fn ensure_arg_count_between(
    inst: &Instruction,
    name: &str,
    min: usize,
    max: usize,
) -> Result<()> {
    if (min..=max).contains(&inst.operands.len()) {
        return Ok(());
    }
    Err(CodegenIrError::invalid_module(format!(
        "{} expected {} to {} args, got {}",
        name,
        min,
        max,
        inst.operands.len()
    )))
}
