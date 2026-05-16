//! Purpose:
//! Emits compiler-extension `ptr_set` pointer operations.
//! Lowers raw address arithmetic, loads, or stores using the target ABI without PHP runtime boxing.
//!
//! Called from:
//! - `crate::codegen::builtins::pointers::emit()`.
//!
//! Key details:
//! - Pointer builtins are elephc extensions and must keep raw memory effects explicit and target-aware.

use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::expr::{coerce_result_to_type, emit_expr, expr_result_heap_ownership};
use crate::codegen::{abi, platform::Arch};
use crate::codegen::context::HeapOwnership;
use crate::parser::ast::{BinOp, Expr, ExprKind};
use crate::types::PhpType;

pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("ptr_set() — write value at pointer address");
    // -- evaluate pointer --
    emit_expr(&args[0], emitter, ctx, data);
    abi::emit_call_label(emitter, "__rt_ptr_check_nonnull");                    // abort with fatal error on null pointer dereference before writing through the pointer
    abi::emit_push_reg(emitter, abi::int_result_reg(emitter));                  // preserve the validated destination pointer while the stored value expression is evaluated

    // -- evaluate value to write --
    let value_ty = emit_expr(&args[1], emitter, ctx, data);
    let release_mixed_after_coerce = matches!(value_ty, PhpType::Mixed | PhpType::Union(_))
        && (expr_result_heap_ownership(&args[1]) == HeapOwnership::Owned
            || matches!(
                args[1].kind,
                ExprKind::BinaryOp {
                    op: BinOp::Add | BinOp::Sub | BinOp::Mul,
                    ..
                }
            ));
    if release_mixed_after_coerce {
        abi::emit_push_reg(emitter, abi::int_result_reg(emitter));              // preserve the boxed Mixed value so it can be released after integer coercion
    }
    coerce_result_to_type(emitter, ctx, data, &value_ty, &PhpType::Int);
    if release_mixed_after_coerce {
        abi::emit_push_reg(emitter, abi::int_result_reg(emitter));              // preserve the coerced integer payload while releasing the temporary Mixed box
        abi::emit_load_temporary_stack_slot(emitter, abi::int_result_reg(emitter), 16);
        abi::emit_decref_if_refcounted(emitter, &PhpType::Mixed);
        abi::emit_pop_reg(emitter, abi::int_result_reg(emitter));               // restore the coerced integer payload after temporary Mixed cleanup
        abi::emit_release_temporary_stack(emitter, 16);
    }

    // -- store value at pointer address --
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction("mov x1, x0");                                  // copy the stored integer payload into a scratch register before restoring the destination pointer on AArch64
            abi::emit_pop_reg(emitter, "x0");                                   // restore the validated destination pointer after evaluating the stored value on AArch64
            emitter.instruction("str x1, [x0]");                                // store one machine-word integer payload through the validated pointer on AArch64
        }
        Arch::X86_64 => {
            emitter.instruction("mov rcx, rax");                                // copy the stored integer payload into a scratch register before restoring the destination pointer on x86_64
            abi::emit_pop_reg(emitter, "rax");                                  // restore the validated destination pointer after evaluating the stored value on x86_64
            emitter.instruction("mov QWORD PTR [rax], rcx");                    // store one machine-word integer payload through the validated pointer on x86_64
        }
    }
    Some(PhpType::Void)
}
