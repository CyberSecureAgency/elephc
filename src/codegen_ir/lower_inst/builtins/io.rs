//! Purpose:
//! Lowers filesystem metadata builtins for the EIR backend.
//! Reuses the shared runtime stat helpers instead of duplicating platform logic.
//!
//! Called from:
//! - `crate::codegen_ir::lower_inst::builtins::lower_builtin_call()`.
//!
//! Key details:
//! - Path operands are already evaluated by EIR and are materialized into the
//!   string result registers expected by the legacy runtime helpers.

use crate::codegen::abi;
use crate::codegen::platform::Arch;
use crate::codegen_ir::{CodegenIrError, Result};
use crate::ir::{Instruction, ValueId};
use crate::types::PhpType;

use super::super::super::context::FunctionContext;
use super::{expect_operand, store_if_result};

/// Lowers `file_put_contents(path, data)` through the target-aware runtime writer.
pub(super) fn lower_file_put_contents(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
) -> Result<()> {
    super::ensure_arg_count(inst, "file_put_contents", 2)?;
    let path = expect_operand(inst, 0)?;
    let data = expect_operand(inst, 1)?;
    match ctx.emitter.target.arch {
        Arch::AArch64 => lower_file_put_contents_arm64(ctx, path, data)?,
        Arch::X86_64 => lower_file_put_contents_x86_64(ctx, path, data)?,
    }
    store_if_result(ctx, inst)
}

/// Lowers `file_exists(path)` through the target-aware runtime stat helper.
pub(super) fn lower_file_exists(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
) -> Result<()> {
    lower_unary_path_predicate(ctx, inst, "file_exists", "__rt_file_exists")
}

/// Lowers `is_file(path)` through the target-aware runtime stat helper.
pub(super) fn lower_is_file(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
) -> Result<()> {
    lower_unary_path_predicate(ctx, inst, "is_file", "__rt_is_file")
}

/// Lowers `is_dir(path)` through the target-aware runtime stat helper.
pub(super) fn lower_is_dir(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
) -> Result<()> {
    lower_unary_path_predicate(ctx, inst, "is_dir", "__rt_is_dir")
}

/// Loads a path string into runtime argument/result registers and stores the boolean result.
fn lower_unary_path_predicate(
    ctx: &mut FunctionContext<'_>,
    inst: &Instruction,
    name: &str,
    runtime_label: &str,
) -> Result<()> {
    super::ensure_arg_count(inst, name, 1)?;
    let path = expect_operand(inst, 0)?;
    require_string(ctx.load_value_to_result(path)?.codegen_repr(), name)?;
    abi::emit_call_label(ctx.emitter, runtime_label);
    store_if_result(ctx, inst)
}

/// Materializes `file_put_contents` arguments for the ARM64 runtime ABI.
fn lower_file_put_contents_arm64(
    ctx: &mut FunctionContext<'_>,
    path: ValueId,
    data: ValueId,
) -> Result<()> {
    load_string_to_result(ctx, path, "file_put_contents filename")?;
    abi::emit_push_reg_pair(ctx.emitter, "x1", "x2");
    load_string_to_result(ctx, data, "file_put_contents data")?;
    ctx.emitter.instruction("mov x3, x1");                                      // pass the data pointer in the runtime helper's second string slot
    ctx.emitter.instruction("mov x4, x2");                                      // pass the data length in the runtime helper's second string slot
    abi::emit_pop_reg_pair(ctx.emitter, "x1", "x2");
    abi::emit_call_label(ctx.emitter, "__rt_file_put_contents");
    Ok(())
}

/// Materializes `file_put_contents` arguments for the Linux x86_64 runtime ABI.
fn lower_file_put_contents_x86_64(
    ctx: &mut FunctionContext<'_>,
    path: ValueId,
    data: ValueId,
) -> Result<()> {
    load_string_to_result(ctx, path, "file_put_contents filename")?;
    abi::emit_push_reg_pair(ctx.emitter, "rax", "rdx");
    load_string_to_result(ctx, data, "file_put_contents data")?;
    ctx.emitter.instruction("mov rdi, rax");                                    // pass the data pointer while the filename remains on the temporary stack
    ctx.emitter.instruction("mov rsi, rdx");                                    // pass the data length while the filename remains on the temporary stack
    abi::emit_pop_reg_pair(ctx.emitter, "rax", "rdx");
    abi::emit_call_label(ctx.emitter, "__rt_file_put_contents");
    Ok(())
}

/// Loads a string SSA value into the target string result registers.
fn load_string_to_result(
    ctx: &mut FunctionContext<'_>,
    value: ValueId,
    context: &str,
) -> Result<()> {
    require_string(ctx.load_value_to_result(value)?.codegen_repr(), context)
}

/// Verifies that a filesystem path argument has the supported string representation.
fn require_string(ty: PhpType, name: &str) -> Result<()> {
    if ty == PhpType::Str {
        return Ok(());
    }
    Err(CodegenIrError::unsupported(format!(
        "{} for PHP type {:?}",
        name,
        ty
    )))
}
