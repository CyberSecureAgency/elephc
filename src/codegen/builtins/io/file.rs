//! Purpose:
//! Emits PHP `file` file input builtin calls.
//! Coordinates path or stream arguments with runtime helpers that allocate returned strings or arrays.
//!
//! Called from:
//! - `crate::codegen::builtins::io::emit()`.
//!
//! Key details:
//! - Failure paths must distinguish PHP false from empty string or empty array results.

use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::expr::emit_expr;
use crate::codegen::abi;
use crate::parser::ast::Expr;
use crate::types::PhpType;

pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("file()");
    emit_expr(&args[0], emitter, ctx, data);
    abi::emit_call_label(emitter, "__rt_file");                                 // call the target-aware runtime helper that reads the file into an array of lines
    Some(PhpType::Array(Box::new(PhpType::Str)))
}
