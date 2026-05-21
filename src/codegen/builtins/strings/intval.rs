//! Purpose:
//! Emits PHP `intval` conversion calls from scalar expressions.
//! Keeps PHP conversion lowering close to string builtins because string parsing is the dominant path.
//!
//! Called from:
//! - `crate::codegen::builtins::strings::emit()`.
//!
//! Key details:
//! - Conversion behavior must stay aligned with type-checker assumptions for scalar-to-int coercion.

use crate::codegen::context::Context;
use crate::codegen::context::HeapOwnership;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::expr::{emit_expr, expr_result_heap_ownership};
use crate::codegen::abi;
use crate::parser::ast::{BinOp, Expr, ExprKind};
use crate::types::PhpType;

pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("intval()");
    let ty = emit_expr(&args[0], emitter, ctx, data);
    match ty {
        PhpType::Str => {
            // -- convert string to integer --
            abi::emit_call_label(emitter, "__rt_atoi");                         // parse the current string result through the target-aware atoi runtime helper
        }
        PhpType::Mixed | PhpType::Union(_) => {
            // -- coerce a boxed Mixed cell to int per PHP's casting rules --
            let release_arg_after_cast = mixed_arg_result_is_owned(&args[0]);
            if release_arg_after_cast {
                abi::emit_push_reg(emitter, abi::int_result_reg(emitter));
            }
            abi::emit_call_label(emitter, "__rt_mixed_cast_int");                // dispatch on the runtime cell tag and return the integer payload (or coerced equivalent)
            if release_arg_after_cast {
                release_preserved_mixed_arg_after_int_cast(emitter);
            }
        }
        _ => {}
    }
    Some(PhpType::Int)
}

fn mixed_arg_result_is_owned(arg: &Expr) -> bool {
    expr_result_heap_ownership(arg) == HeapOwnership::Owned
        || matches!(
            arg.kind,
            ExprKind::BinaryOp {
                op: BinOp::Add | BinOp::Sub | BinOp::Mul,
                ..
            }
        )
}

fn release_preserved_mixed_arg_after_int_cast(emitter: &mut Emitter) {
    abi::emit_push_reg(emitter, abi::int_result_reg(emitter));
    abi::emit_load_temporary_stack_slot(emitter, abi::int_result_reg(emitter), 16);
    abi::emit_decref_if_refcounted(emitter, &PhpType::Mixed);
    abi::emit_pop_reg(emitter, abi::int_result_reg(emitter));
    abi::emit_release_temporary_stack(emitter, 16);
}
