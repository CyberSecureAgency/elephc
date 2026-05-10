//! Purpose:
//! Emits PHP `boolval` type conversion or type-name builtin calls.
//! Applies PHP scalar conversion rules or materializes runtime type names for values.
//!
//! Called from:
//! - `crate::codegen::builtins::types::emit()`.
//!
//! Key details:
//! - Conversion results must stay aligned with type-checker signatures and boxed Mixed handling.

use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::expr::{coerce_to_truthiness, emit_expr};
use crate::parser::ast::Expr;
use crate::types::PhpType;

pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("boolval()");
    // -- convert any value to boolean (truthy/falsy) --
    let src_ty = emit_expr(&args[0], emitter, ctx, data);
    coerce_to_truthiness(emitter, ctx, &src_ty);                                // normalize the value to PHP truthiness through the shared target-aware coercion helper
    Some(PhpType::Bool)
}
