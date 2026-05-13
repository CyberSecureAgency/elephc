//! Purpose:
//! Emits PHP `getenv` environment/platform information builtin calls.
//! Delegates host environment lookup or platform string construction to runtime helpers.
//!
//! Called from:
//! - `crate::codegen::builtins::system::emit()`.
//!
//! Key details:
//! - Environment and platform state are observable and must not be folded as compile-time constants here.

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
    emitter.comment("getenv()");
    // -- evaluate the environment variable name string --
    emit_expr(&args[0], emitter, ctx, data);
    // -- convert to C string and call getenv --
    abi::emit_call_label(emitter, "__rt_getenv");                               // get env var through the target-aware runtime helper → ptr/len result regs
    Some(PhpType::Str)
}
