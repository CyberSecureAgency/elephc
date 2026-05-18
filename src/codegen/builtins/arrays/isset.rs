//! Purpose:
//! Emits PHP `isset` checks without evaluating to ordinary truthiness.
//! Owns null/unset sentinel handling for variables and array element probes.
//!
//! Called from:
//! - `crate::codegen::builtins::arrays::emit()`.
//!
//! Key details:
//! - Must distinguish PHP null/unset semantics from false, zero, and empty string values.

use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::expr::emit_expr;
use crate::codegen::{abi, platform::Arch};
use crate::parser::ast::{Expr, ExprKind};
use crate::types::PhpType;

const NULL_SENTINEL: i64 = 0x7fff_ffff_ffff_fffe;

pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("isset()");
    if let ExprKind::ArrayAccess { array, index } = &args[0].kind {
        if emit_array_access_isset(&args[0], array, index, emitter, ctx, data) {
            return Some(PhpType::Int);
        }
    }

    let ty = emit_expr(&args[0], emitter, ctx, data);
    emit_not_null_result(&ty, emitter);

    Some(PhpType::Int)
}

fn emit_array_access_isset(
    access: &Expr,
    array: &Expr,
    index: &Expr,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> bool {
    let array_ty = crate::codegen::functions::infer_contextual_type(array, ctx);
    if crate::codegen::expr::arrays::type_is_array_access_object(&array_ty, ctx) {
        crate::codegen::expr::arrays::emit_array_access_offset_exists(
            array, index, emitter, ctx, data,
        );
        return true;
    }

    match array_ty.codegen_repr() {
        PhpType::Str => {
            emit_expr(access, emitter, ctx, data);
            emit_string_offset_isset_result(emitter);
            true
        }
        PhpType::Array(elem_ty) => match elem_ty.codegen_repr() {
            PhpType::Mixed => false,
            PhpType::Void => {
                emit_array_and_index_then_false(array, index, emitter, ctx, data);
                true
            }
            _ => {
                emit_indexed_offset_exists(array, index, emitter, ctx, data);
                true
            }
        },
        PhpType::AssocArray { value, .. } => match value.codegen_repr() {
            PhpType::Mixed => false,
            PhpType::Void => {
                emit_array_and_index_then_false(array, index, emitter, ctx, data);
                true
            }
            _ => {
                emit_assoc_offset_exists(array, index, emitter, ctx, data);
                true
            }
        },
        _ => false,
    }
}

fn emit_indexed_offset_exists(
    array: &Expr,
    index: &Expr,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) {
    emit_expr(array, emitter, ctx, data);
    abi::emit_push_reg(emitter, abi::int_result_reg(emitter));                 // preserve the indexed-array pointer while evaluating the offset expression
    emit_expr(index, emitter, ctx, data);
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction("mov x1, x0");                                  // move the offset into the indexed-array key-exists helper argument
            abi::emit_pop_reg(emitter, "x0");                                   // restore the indexed-array pointer into the helper receiver argument
        }
        Arch::X86_64 => {
            emitter.instruction("mov rsi, rax");                                // move the offset into the indexed-array key-exists helper argument
            abi::emit_pop_reg(emitter, "rdi");                                  // restore the indexed-array pointer into the helper receiver argument
        }
    }
    abi::emit_call_label(emitter, "__rt_array_key_exists");                    // return whether the indexed offset lies inside array bounds
}

fn emit_assoc_offset_exists(
    array: &Expr,
    index: &Expr,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) {
    emit_expr(array, emitter, ctx, data);
    abi::emit_push_reg(emitter, abi::int_result_reg(emitter));                 // preserve the hash-table pointer while evaluating the offset expression
    crate::codegen::emit_normalized_hash_key(index, emitter, ctx, data);
    let (key_ptr_reg, key_len_reg) = abi::string_result_regs(emitter);
    abi::emit_push_reg_pair(emitter, key_ptr_reg, key_len_reg);                // preserve the normalized key while restoring the hash-table pointer
    match emitter.target.arch {
        Arch::AArch64 => {
            abi::emit_pop_reg_pair(emitter, "x1", "x2");                       // restore the normalized key into hash-get argument registers
            abi::emit_pop_reg(emitter, "x0");                                   // restore the hash-table pointer into the hash-get receiver argument
        }
        Arch::X86_64 => {
            abi::emit_pop_reg_pair(emitter, "rsi", "rdx");                     // restore the normalized key into hash-get argument registers
            abi::emit_pop_reg(emitter, "rdi");                                  // restore the hash-table pointer into the hash-get receiver argument
        }
    }
    abi::emit_call_label(emitter, "__rt_hash_get");                            // return the hash lookup found flag for non-null typed values
}

fn emit_array_and_index_then_false(
    array: &Expr,
    index: &Expr,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) {
    emit_expr(array, emitter, ctx, data);
    emit_expr(index, emitter, ctx, data);
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction("mov x0, #0");                                  // null-only offsets are never set
        }
        Arch::X86_64 => {
            emitter.instruction("xor eax, eax");                                // null-only offsets are never set
        }
    }
}

fn emit_not_null_result(ty: &PhpType, emitter: &mut Emitter) {
    match ty.codegen_repr() {
        PhpType::Void => match emitter.target.arch {
            Arch::AArch64 => {
                emitter.instruction("mov x0, #0");                              // null values are not set
            }
            Arch::X86_64 => {
                emitter.instruction("xor eax, eax");                            // null values are not set
            }
        },
        PhpType::Mixed => {
            abi::emit_call_label(emitter, "__rt_mixed_unbox");                  // inspect the boxed value before applying PHP isset null semantics
            match emitter.target.arch {
                Arch::AArch64 => {
                    emitter.instruction("cmp x0, #8");                          // runtime tag 8 is PHP null
                    emitter.instruction("cset x0, ne");                         // isset is true only when the boxed payload is not null
                }
                Arch::X86_64 => {
                    emitter.instruction("cmp rax, 8");                          // runtime tag 8 is PHP null
                    emitter.instruction("setne al");                            // set the boolean byte when the boxed payload is not null
                    emitter.instruction("movzx rax, al");                       // widen the isset result into the canonical integer register
                }
            }
        }
        PhpType::Int | PhpType::Bool => match emitter.target.arch {
            Arch::AArch64 => {
                abi::emit_load_int_immediate(emitter, "x9", NULL_SENTINEL);
                emitter.instruction("cmp x0, x9");                              // compare the scalar result against the shared null sentinel
                emitter.instruction("cset x0, ne");                             // isset is true only when the scalar result is not null
            }
            Arch::X86_64 => {
                abi::emit_load_int_immediate(emitter, "r10", NULL_SENTINEL);
                emitter.instruction("cmp rax, r10");                            // compare the scalar result against the shared null sentinel
                emitter.instruction("setne al");                                // set the boolean byte when the scalar result is not null
                emitter.instruction("movzx rax, al");                           // widen the isset result into the canonical integer register
            }
        },
        _ => match emitter.target.arch {
            Arch::AArch64 => {
                emitter.instruction("mov x0, #1");                              // non-nullable compiled values are set
            }
            Arch::X86_64 => {
                emitter.instruction("mov rax, 1");                              // non-nullable compiled values are set
            }
        },
    }
}

fn emit_string_offset_isset_result(emitter: &mut Emitter) {
    let (_, len_reg) = abi::string_result_regs(emitter);
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction(&format!("cmp {}, #0", len_reg));               // check whether string offset access produced a character
            emitter.instruction("cset x0, ne");                                 // return true only when the string offset is in bounds
        }
        Arch::X86_64 => {
            emitter.instruction(&format!("cmp {}, 0", len_reg));                // check whether string offset access produced a character
            emitter.instruction("setne al");                                    // return true only when the string offset is in bounds
            emitter.instruction("movzx eax, al");                               // widen the boolean byte into the canonical integer result
        }
    }
}
