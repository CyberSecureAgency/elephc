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
use crate::codegen_ir::{CodegenIrError, Result};
use crate::ir::Instruction;
use crate::types::PhpType;

use super::super::super::context::FunctionContext;
use super::{expect_operand, store_if_result};

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
