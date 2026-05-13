//! Purpose:
//! Emits PHP `fstat` stream builtin calls over runtime file handles.
//! Uses shared stream unboxing before invoking file descriptor runtime helpers.
//!
//! Called from:
//! - `crate::codegen::builtins::io::emit()`.
//!
//! Key details:
//! - Stream resources must be validated and failure results must follow PHP false/null conventions.

use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::abi;
use crate::parser::ast::Expr;
use crate::types::PhpType;
use super::stat_result::box_stat_array_or_false_result;
use super::stream_arg::emit_stream_fd_arg;

pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("fstat()");
    emit_stream_fd_arg("fstat", &args[0], emitter, ctx, data);
    abi::emit_call_label(emitter, "__rt_fstat_array");                          // call the target-aware runtime helper that builds the PHP-compatible fstat array
    box_stat_array_or_false_result(emitter, ctx);
    Some(PhpType::Mixed)
}
