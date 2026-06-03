//! Purpose:
//! Emits PHP `long2ip` calls.
//! Formats a 32-bit integer as a dotted-quad IPv4 string.
//!
//! Called from:
//! - `crate::codegen::builtins::strings::emit()`.
//!
//! Key details:
//! - Delegates the formatting to the `__rt_long2ip` runtime helper, which
//!   leaves the result string in the standard pointer/length registers.

use crate::codegen::abi;
use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::expr::emit_expr;
use crate::codegen::platform::Arch;
use crate::parser::ast::Expr;
use crate::types::PhpType;

/// Emits codegen for PHP `long2ip()` string builtin calls.
pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("long2ip()");
    emit_expr(&args[0], emitter, ctx, data);
    if emitter.target.arch == Arch::X86_64 {
        emitter.instruction("mov rdi, rax");                                    // move the IP integer into the runtime-helper argument register
    }
    abi::emit_call_label(emitter, "__rt_long2ip");
    Some(PhpType::Str)
}
