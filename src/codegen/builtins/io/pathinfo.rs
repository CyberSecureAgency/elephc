use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::expr::emit_expr;
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
    emitter.comment("pathinfo()");
    emit_expr(&args[0], emitter, ctx, data);
    if args.len() == 1 {
        // No-flag form: build the associative array via the runtime helper.
        abi::emit_call_label(emitter, "__rt_pathinfo_array");                   // call the runtime helper that builds the dirname/basename/extension/filename hash
        // The hash pointer comes back in x0 / rax — that is already the
        // standard integer-result register used everywhere else for hash-typed
        // expression results.
        return Some(PhpType::AssocArray {
            key: Box::new(PhpType::Str),
            value: Box::new(PhpType::Str),
        });
    }
    // Single-flag form.
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction("stp x1, x2, [sp, #-16]!");                     // preserve the path ptr/len while the flag expression is evaluated
            emit_expr(&args[1], emitter, ctx, data);
            emitter.instruction("mov x3, x0");                                  // move the flag value into the runtime's flag register
            emitter.instruction("ldp x1, x2, [sp], #16");                       // restore the path ptr/len after evaluating the flag expression
        }
        Arch::X86_64 => {
            abi::emit_push_reg_pair(emitter, "rax", "rdx");                     // preserve the path ptr/len while the flag expression is evaluated
            emit_expr(&args[1], emitter, ctx, data);
            emitter.instruction("mov rdi, rax");                                // move the flag value into the x86_64 runtime flag register
            abi::emit_pop_reg_pair(emitter, "rax", "rdx");                      // restore the path ptr/len after evaluating the flag expression
        }
    }
    abi::emit_call_label(emitter, "__rt_pathinfo_str");                         // call the target-aware single-flag runtime helper that returns the requested component
    Some(PhpType::Str)
}
