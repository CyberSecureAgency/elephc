//! Purpose:
//! Emits PHP `trim` string transformation or formatting calls.
//! Marshals string/scalar arguments into runtime helpers that allocate returned PHP strings.
//!
//! Called from:
//! - `crate::codegen::builtins::strings::emit()`.
//!
//! Key details:
//! - Returned string pointer/length pairs must be treated as owned runtime values when the helper allocates.

use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::{abi, platform::Arch};
use crate::parser::ast::Expr;
use crate::types::PhpType;

pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("trim()");

    if args.len() == 1 {
        super::args::emit_string_arg(&args[0], emitter, ctx, data);
        abi::emit_call_label(emitter, "__rt_trim");                             // strip whitespace from both ends through the target-aware trim runtime helper
    } else {
        super::args::emit_string_arg(&args[0], emitter, ctx, data);
        match emitter.target.arch {
            Arch::AArch64 => {
                emitter.instruction("str x1, [sp, #-16]!");                     // push the source string pointer while the trim mask expression is evaluated
                emitter.instruction("str x2, [sp, #-16]!");                     // push the source string length while the trim mask expression is evaluated
                super::args::emit_string_arg(&args[1], emitter, ctx, data);
                emitter.instruction("mov x3, x1");                              // move the trim mask pointer into the secondary trim-mask runtime string-argument pair
                emitter.instruction("mov x4, x2");                              // move the trim mask length into the secondary trim-mask runtime string-argument pair
                emitter.instruction("ldr x2, [sp], #16");                       // restore the source string length after evaluating the trim mask expression
                emitter.instruction("ldr x1, [sp], #16");                       // restore the source string pointer after evaluating the trim mask expression
            }
            Arch::X86_64 => {
                abi::emit_push_reg_pair(emitter, "rax", "rdx");                 // preserve the source string ptr/len while the trim mask expression is evaluated
                super::args::emit_string_arg(&args[1], emitter, ctx, data);
                emitter.instruction("mov rdi, rax");                            // move the trim mask pointer into the secondary x86_64 trim-mask runtime string-argument slot
                emitter.instruction("mov rsi, rdx");                            // move the trim mask length into the secondary x86_64 trim-mask runtime string-argument slot
                abi::emit_pop_reg_pair(emitter, "rax", "rdx");                  // restore the source string ptr/len after evaluating the trim mask expression
            }
        }
        abi::emit_call_label(emitter, "__rt_trim_mask");                        // strip the requested character mask from both sides through the target-aware trim runtime helper
    }

    Some(PhpType::Str)
}
