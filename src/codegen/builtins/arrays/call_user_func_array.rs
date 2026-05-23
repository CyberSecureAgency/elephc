//! Purpose:
//! Emits PHP `call_user_func_array` builtin calls that invoke user-provided callbacks.
//! Owns callback argument materialization, result shape selection, and runtime helper calls.
//!
//! Called from:
//! - `crate::codegen::builtins::arrays::emit()`.
//!
//! Key details:
//! - Callback lowering must preserve PHP source evaluation order, captures, and callable return ownership.

use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::expr::emit_expr;
use crate::codegen::expr::calls::args;
use crate::codegen::platform::Arch;
use crate::codegen::abi;
use crate::parser::ast::{Expr, ExprKind};
use crate::types::{FunctionSig, PhpType};
use super::callback_env;
use super::super::callable_lookup::{lookup_function, FunctionLookup};

const UNKNOWN_CALLBACK_ARG_LIMIT: usize = 16;

fn emit_array_value_type_stamp(emitter: &mut Emitter, array_reg: &str, elem_ty: &PhpType) {
    let value_type_tag = match elem_ty {
        PhpType::Float => 2,
        PhpType::Bool => 3,
        PhpType::Str => 1,
        PhpType::Array(_) => 4,
        PhpType::AssocArray { .. } => 5,
        PhpType::Object(_) => 6,
        PhpType::Mixed => 7,
        PhpType::Union(_) => 7,
        PhpType::Void => 8,
        _ => return,
    };
    match emitter.target.arch {
        crate::codegen::platform::Arch::AArch64 => {
            emitter.instruction(&format!("ldr x10, [{}, #-8]", array_reg));     // load the packed array kind word from the heap header
            emitter.instruction("mov x12, #0x80ff");                            // preserve the indexed-array kind and persistent COW flag
            emitter.instruction("and x10, x10, x12");                           // keep only the persistent indexed-array metadata bits
            emitter.instruction(&format!("mov x11, #{}", value_type_tag));      // materialize the runtime array value_type tag
            emitter.instruction("lsl x11, x11, #8");                            // move the value_type tag into the packed kind-word byte lane
            emitter.instruction("orr x10, x10, x11");                           // combine the heap kind with the array value_type tag
            emitter.instruction(&format!("str x10, [{}, #-8]", array_reg));     // persist the packed array kind word in the heap header
        }
        crate::codegen::platform::Arch::X86_64 => {
            emitter.instruction(&format!("mov r10, QWORD PTR [{} - 8]", array_reg)); // load the packed array kind word from the heap header
            emitter.instruction("mov rdx, 0xffffffff000080ff");                 // materialize the x86_64 indexed-array metadata preservation mask
            emitter.instruction("and r10, rdx");                                // preserve heap marker, indexed-array kind, and persistent COW bits
            emitter.instruction(&format!("mov rcx, {}", value_type_tag));       // materialize the runtime array value_type tag
            emitter.instruction("shl rcx, 8");                                  // move the value_type tag into the packed kind-word byte lane
            emitter.instruction("or r10, rcx");                                 // combine the heap kind with the array value_type tag
            emitter.instruction(&format!("mov QWORD PTR [{} - 8], r10", array_reg)); // persist the packed array kind word in the heap header
        }
    }
}

pub(crate) enum LoadedArraySource {
    Result,
    TemporaryStackSlot(usize),
}

pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("call_user_func_array()");
    if let (ExprKind::StringLiteral(name), ExprKind::ArrayLiteral(elems)) =
        (&args[0].kind, &args[1].kind)
    {
        match lookup_function(ctx, name) {
            Some(FunctionLookup::Extern(extern_name)) => {
                return Some(crate::codegen::ffi::emit_extern_call(
                    &extern_name,
                    elems,
                    args[0].span,
                    emitter,
                    ctx,
                    data,
                ));
            }
            Some(FunctionLookup::Builtin(builtin_name)) => {
                if let Some(ret_ty) = crate::codegen::builtins::emit_builtin_call(
                    &builtin_name,
                    elems,
                    args[0].span,
                    emitter,
                    ctx,
                    data,
                ) {
                    return Some(ret_ty);
                }
            }
            Some(FunctionLookup::UserFunction(_)) | Some(FunctionLookup::IncludeVariant(_)) | None => {}
        }
    }
    let save_concat_before_args =
        emitter.target.arch == crate::codegen::platform::Arch::X86_64;
    if save_concat_before_args {
        crate::codegen::expr::save_concat_offset_before_nested_call(emitter, ctx);
    }
    let call_reg = abi::nested_call_reg(emitter);

    // -- resolve callback function address and signature --
    let precomputed_sig = crate::codegen::callables::callable_sig(&args[0], ctx);
    let captures =
        callback_env::materialize_callback_address(&args[0], call_reg, emitter, ctx, data);
    let sig = if matches!(
        &args[0].kind,
        ExprKind::Closure { .. } | ExprKind::FirstClassCallable(_)
    ) {
        Some(ctx.deferred_closures
            .last()
            .expect("call_user_func_array: missing synthesized callable signature")
            .sig
            .clone())
    } else {
        precomputed_sig
    };

    let ret_ty = if let Some(sig) = sig {
        if sig.ref_params.iter().any(|is_ref| *is_ref) {
            let arr_ty = emit_expr(&args[1], emitter, ctx, data);
            let literal_arg_elems = match &args[1].kind {
                ExprKind::ArrayLiteral(elems) => Some(elems.as_slice()),
                _ => None,
            };
            emit_loaded_array_callback_call(
                LoadedArraySource::Result,
                &arr_ty,
                literal_arg_elems,
                call_reg,
                &captures,
                &sig,
                save_concat_before_args,
                emitter,
                ctx,
                data,
            )
        } else {
            emit_spread_callback_call_from_array_expr(
                &args[1],
                call_reg,
                &captures,
                &sig,
                save_concat_before_args,
                emitter,
                ctx,
                data,
            )
        }
    } else {
        // Evaluate the array argument (second arg)
        let arr_ty = emit_expr(&args[1], emitter, ctx, data);
        emit_loaded_array_unknown_callback_call(
            LoadedArraySource::Result,
            &arr_ty,
            call_reg,
            &captures,
            save_concat_before_args,
            emitter,
            ctx,
            data,
        )
    };

    Some(ret_ty)
}

fn emit_spread_callback_call_from_array_expr(
    arg_array: &Expr,
    call_reg: &str,
    captures: &[(String, PhpType, bool)],
    sig: &FunctionSig,
    concat_saved_before_args: bool,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> PhpType {
    abi::emit_push_reg(emitter, call_reg);                                      // preserve the callback address while unpacking call_user_func_array() args
    let spread_arg = Expr::new(ExprKind::Spread(Box::new(arg_array.clone())), arg_array.span);
    let visible_param_count = sig.params.len();
    let regular_param_count = if sig.variadic.is_some() {
        visible_param_count.saturating_sub(1)
    } else {
        visible_param_count
    };
    let emitted_args = args::emit_pushed_call_args(
        &[spread_arg],
        Some(sig),
        regular_param_count,
        "call_user_func_array ref arg",
        true,
        true,
        emitter,
        ctx,
        data,
    );
    let mut arg_types = emitted_args.arg_types;
    callback_env::push_captures_as_hidden_args(captures, emitter, ctx, &mut arg_types);

    let callback_offset = args::pushed_temp_bytes(&arg_types) + emitted_args.source_temp_bytes;
    abi::emit_load_temporary_stack_slot(emitter, call_reg, callback_offset);
    let assignments = abi::build_outgoing_arg_assignments_for_target(emitter.target, &arg_types, 0);
    let overflow_bytes = abi::materialize_outgoing_args(emitter, &assignments);
    let ret_ty = sig.return_type.clone();

    if !concat_saved_before_args {
        crate::codegen::expr::save_concat_offset_before_nested_call(emitter, ctx);
    }
    abi::emit_call_reg(emitter, call_reg);
    if concat_saved_before_args {
        abi::emit_release_temporary_stack(emitter, overflow_bytes);
        abi::emit_release_temporary_stack(emitter, emitted_args.source_temp_bytes);
        abi::emit_release_temporary_stack(emitter, 16);
        crate::codegen::expr::restore_concat_offset_after_nested_call(emitter, ctx, &ret_ty);
    } else {
        crate::codegen::expr::restore_concat_offset_after_nested_call(emitter, ctx, &ret_ty);
        abi::emit_release_temporary_stack(emitter, overflow_bytes);
        abi::emit_release_temporary_stack(emitter, emitted_args.source_temp_bytes);
        abi::emit_release_temporary_stack(emitter, 16);
    }

    ret_ty
}

pub(crate) fn emit_loaded_array_callback_call(
    array_source: LoadedArraySource,
    arr_ty: &PhpType,
    literal_arg_elems: Option<&[Expr]>,
    call_reg: &str,
    captures: &[(String, PhpType, bool)],
    sig: &FunctionSig,
    concat_saved_before_args: bool,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> PhpType {
    if matches!(arr_ty, PhpType::AssocArray { .. }) {
        return emit_loaded_assoc_array_callback_call(
            array_source,
            arr_ty,
            call_reg,
            captures,
            sig,
            concat_saved_before_args,
            emitter,
            ctx,
            data,
        );
    }

    let (array_reg, len_reg, tail_count_reg, tail_index_reg, index_reg, offset_reg, data_reg, peek_reg, array_new_capacity_reg, array_new_elem_size_reg, len_store_reg) =
        match emitter.target.arch {
            crate::codegen::platform::Arch::AArch64 => (
                "x20", "x21", "x22", "x23", "x24", "x25", "x26", "x9", "x0", "x1", "x10"
            ),
            crate::codegen::platform::Arch::X86_64 => (
                "r13", "r14", "r15", "rbx", "rcx", "r8", "r9", "r11", "rdi", "rsi", "r10"
            ),
        };

    // Determine element type and size from the array type
    let elem_ty = match arr_ty {
        PhpType::Array(t) => *t.clone(),
        PhpType::AssocArray { value, .. } => *value.clone(),
        _ => PhpType::Int,
    };
    let elem_size = args::array_element_stride(&elem_ty);

    match array_source {
        LoadedArraySource::Result => {
            emitter.instruction(&format!("mov {}, {}", array_reg, abi::int_result_reg(emitter))); // preserve the callback-argument array pointer across element boxing
        }
        LoadedArraySource::TemporaryStackSlot(offset) => {
            abi::emit_load_temporary_stack_slot(emitter, array_reg, offset);
        }
    }
    abi::emit_load_from_address(emitter, len_reg, array_reg, 0);                // load callback-argument array length

    // -- extract elements from array and push them as regular call arguments --
    let mut arg_types = Vec::new();
    let visible_param_count = sig.params.len();
    let regular_param_count = if sig.variadic.is_some() {
        visible_param_count.saturating_sub(1)
    } else {
        visible_param_count
    };
    for i in 0..regular_param_count {
        let is_ref = sig.ref_params.get(i).copied().unwrap_or(false);
        if is_ref {
            if let Some(Expr {
                kind: ExprKind::Variable(var_name),
                ..
            }) = literal_arg_elems.and_then(|elems| elems.get(i))
            {
                if !args::emit_ref_arg_variable_address(
                    var_name,
                    "call_user_func_array ref arg",
                    emitter,
                    ctx,
                ) {
                    panic!("call_user_func_array() by-reference callback argument variable not found");
                }
                args::push_arg_value(emitter, &PhpType::Int);
                arg_types.push(PhpType::Int);
                continue;
            }
        }
        let has_default = sig.defaults.get(i).and_then(|d| d.as_ref()).is_some();
        let target_ty = if args::declared_target_ty(Some(&sig), i).is_some() || has_default {
            sig.params.get(i).map(|(_, ty)| ty)
        } else {
            None
        };
        let pushed_ty = target_ty
            .map(PhpType::codegen_repr)
            .unwrap_or_else(|| elem_ty.codegen_repr());

        if let Some(default_expr) = sig.defaults.get(i).and_then(|d| d.as_ref()) {
            let load_label = ctx.next_label("cufa_load_arg");
            let done_label = ctx.next_label("cufa_arg_done");
            match emitter.target.arch {
                crate::codegen::platform::Arch::AArch64 => {
                    emitter.instruction(&format!("cmp {}, #{}", len_reg, i + 1)); // compare provided array length against required positional index
                }
                crate::codegen::platform::Arch::X86_64 => {
                    emitter.instruction(&format!("cmp {}, {}", len_reg, i + 1)); // compare provided array length against required positional index
                }
            }
            match emitter.target.arch {
                crate::codegen::platform::Arch::AArch64 => {
                    emitter.instruction(&format!("b.ge {}", load_label));       // load an explicit array element when present
                }
                crate::codegen::platform::Arch::X86_64 => {
                    emitter.instruction(&format!("jge {}", load_label));        // load an explicit array element when present
                }
            }
            let _ = args::push_expr_arg(default_expr, target_ty, emitter, ctx, data);
            abi::emit_jump(emitter, &done_label);
            emitter.label(&load_label);
            args::load_array_element_to_result(emitter, &elem_ty, array_reg, 24 + i * elem_size);
            let _ =
                args::push_loaded_array_element_arg(&elem_ty, target_ty, emitter, ctx, data);
            emitter.label(&done_label);
        } else {
            args::load_array_element_to_result(emitter, &elem_ty, array_reg, 24 + i * elem_size);
            let _ =
                args::push_loaded_array_element_arg(&elem_ty, target_ty, emitter, ctx, data);
        }
        arg_types.push(pushed_ty);
    }

    if sig.variadic.is_some() {
        let variadic_elem_ty = sig
            .params
            .get(visible_param_count.saturating_sub(1))
            .and_then(|(_, ty)| match ty {
                PhpType::Array(elem) => Some((**elem).clone()),
                _ => None,
            })
            .unwrap_or_else(|| elem_ty.clone());
        let build_label = ctx.next_label("cufa_build_variadic");
        let done_label = ctx.next_label("cufa_variadic_done");
        match emitter.target.arch {
            crate::codegen::platform::Arch::AArch64 => {
                emitter.instruction(&format!("cmp {}, #{}", len_reg, regular_param_count)); // compare provided array length against the fixed arity prefix
            }
            crate::codegen::platform::Arch::X86_64 => {
                emitter.instruction(&format!("cmp {}, {}", len_reg, regular_param_count)); // compare provided array length against the fixed arity prefix
            }
        }
        match emitter.target.arch {
            crate::codegen::platform::Arch::AArch64 => {
                emitter.instruction(&format!("b.gt {}", build_label));          // build a tail array only when extra positional elements exist
            }
            crate::codegen::platform::Arch::X86_64 => {
                emitter.instruction(&format!("jg {}", build_label));            // build a tail array only when extra positional elements exist
            }
        }
        emitter.comment("empty variadic array for call_user_func_array()");
        abi::emit_load_int_immediate(emitter, array_new_capacity_reg, 4);
        abi::emit_load_int_immediate(emitter, array_new_elem_size_reg, variadic_elem_ty.stack_size() as i64);
        abi::emit_call_label(emitter, "__rt_array_new");
        abi::emit_push_reg(emitter, abi::int_result_reg(emitter));              // push the empty variadic array onto the temporary arg stack
        abi::emit_jump(emitter, &done_label);

        emitter.label(&build_label);
        match emitter.target.arch {
            crate::codegen::platform::Arch::AArch64 => {
                emitter.instruction(&format!("sub {}, {}, #{}", tail_count_reg, len_reg, regular_param_count)); // compute the count of variadic tail arguments
            }
            crate::codegen::platform::Arch::X86_64 => {
                emitter.instruction(&format!("mov {}, {}", tail_count_reg, len_reg)); // seed the tail count from the provided array length
                emitter.instruction(&format!("sub {}, {}", tail_count_reg, regular_param_count)); // compute the count of variadic tail arguments
            }
        }
        emitter.instruction(&format!("mov {}, {}", array_new_capacity_reg, tail_count_reg)); // pass the exact tail argument count as the initial capacity
        abi::emit_load_int_immediate(emitter, array_new_elem_size_reg, variadic_elem_ty.stack_size() as i64);
        abi::emit_call_label(emitter, "__rt_array_new");
        abi::emit_push_reg(emitter, abi::int_result_reg(emitter));              // keep the variadic array pointer on the stack while filling it
        emitter.instruction(&format!("mov {}, {}", peek_reg, abi::int_result_reg(emitter))); // copy the variadic array pointer into a scratch register for metadata stamping
        emit_array_value_type_stamp(emitter, peek_reg, &variadic_elem_ty);      // stamp the array header with the variadic element runtime tag
        abi::emit_load_int_immediate(emitter, tail_index_reg, 0);
        let loop_label = ctx.next_label("cufa_variadic_loop");
        let loop_done_label = ctx.next_label("cufa_variadic_loop_done");
        emitter.label(&loop_label);
        emitter.instruction(&format!("cmp {}, {}", tail_index_reg, tail_count_reg)); // stop once every tail element has been copied
        match emitter.target.arch {
            crate::codegen::platform::Arch::AArch64 => {
                emitter.instruction(&format!("b.ge {}", loop_done_label));      // exit the fill loop when the tail array is complete
            }
            crate::codegen::platform::Arch::X86_64 => {
                emitter.instruction(&format!("jge {}", loop_done_label));       // exit the fill loop when the tail array is complete
            }
        }
        emitter.instruction(&format!("mov {}, {}", index_reg, tail_index_reg)); // copy the tail index into a scratch register
        if regular_param_count > 0 {
            match emitter.target.arch {
                crate::codegen::platform::Arch::AArch64 => {
                    emitter.instruction(&format!("add {}, {}, #{}", index_reg, index_reg, regular_param_count)); // offset the tail index by the fixed-arity prefix length
                }
                crate::codegen::platform::Arch::X86_64 => {
                    emitter.instruction(&format!("add {}, {}", index_reg, regular_param_count)); // offset the tail index by the fixed-arity prefix length
                }
            }
        }
        emitter.instruction(&format!("mov {}, {}", data_reg, array_reg));       // start from the callback-argument array pointer before indexing into payload data
        match emitter.target.arch {
            crate::codegen::platform::Arch::AArch64 => {
                emitter.instruction(&format!("add {}, {}, #24", data_reg, data_reg)); // skip the fixed array header before indexing variadic source elements
            }
            crate::codegen::platform::Arch::X86_64 => {
                emitter.instruction(&format!("add {}, 24", data_reg));          // skip the fixed array header before indexing variadic source elements
            }
        }
        match elem_ty.codegen_repr() {
            PhpType::Str => {
                match emitter.target.arch {
                    crate::codegen::platform::Arch::AArch64 => {
                        emitter.instruction(&format!("lsl {}, {}, #4", offset_reg, index_reg)); // compute the 16-byte source slot offset for a string element
                        emitter.instruction(&format!("add {}, {}, {}", data_reg, data_reg, offset_reg)); // advance to the selected source string element
                        let (ptr_reg, len_reg_out) = abi::string_result_regs(emitter);
                        abi::emit_load_from_address(emitter, ptr_reg, data_reg, 0);
                        abi::emit_load_from_address(emitter, len_reg_out, data_reg, 8);
                    }
                    crate::codegen::platform::Arch::X86_64 => {
                        emitter.instruction(&format!("mov {}, {}", offset_reg, index_reg)); // copy the element index before scaling to bytes
                        emitter.instruction(&format!("shl {}, 4", offset_reg)); // compute the 16-byte source slot offset for a string element
                        emitter.instruction(&format!("add {}, {}", data_reg, offset_reg)); // advance to the selected source string element
                        let (ptr_reg, len_reg_out) = abi::string_result_regs(emitter);
                        abi::emit_load_from_address(emitter, ptr_reg, data_reg, 0);
                        abi::emit_load_from_address(emitter, len_reg_out, data_reg, 8);
                    }
                }
            }
            PhpType::Float => {
                match emitter.target.arch {
                    crate::codegen::platform::Arch::AArch64 => {
                        emitter.instruction(&format!("lsl {}, {}, #3", offset_reg, index_reg)); // compute the 8-byte source slot offset for a float element
                        emitter.instruction(&format!("add {}, {}, {}", data_reg, data_reg, offset_reg)); // advance to the selected source float element
                    }
                    crate::codegen::platform::Arch::X86_64 => {
                        emitter.instruction(&format!("mov {}, {}", offset_reg, index_reg)); // copy the element index before scaling to bytes
                        emitter.instruction(&format!("shl {}, 3", offset_reg)); // compute the 8-byte source slot offset for a float element
                        emitter.instruction(&format!("add {}, {}", data_reg, offset_reg)); // advance to the selected source float element
                    }
                }
                abi::emit_load_from_address(emitter, abi::float_result_reg(emitter), data_reg, 0);
            }
            PhpType::Void => {}
            _ => {
                match emitter.target.arch {
                    crate::codegen::platform::Arch::AArch64 => {
                        emitter.instruction(&format!("lsl {}, {}, #3", offset_reg, index_reg)); // compute the 8-byte source slot offset for a scalar or boxed element
                        emitter.instruction(&format!("add {}, {}, {}", data_reg, data_reg, offset_reg)); // advance to the selected source scalar element
                    }
                    crate::codegen::platform::Arch::X86_64 => {
                        emitter.instruction(&format!("mov {}, {}", offset_reg, index_reg)); // copy the element index before scaling to bytes
                        emitter.instruction(&format!("shl {}, 3", offset_reg)); // compute the 8-byte source slot offset for a scalar or boxed element
                        emitter.instruction(&format!("add {}, {}", data_reg, offset_reg)); // advance to the selected source scalar element
                    }
                }
                abi::emit_load_from_address(emitter, abi::int_result_reg(emitter), data_reg, 0);
            }
        }
        let (stored_ty, boxed_to_mixed) = args::coerce_current_value_to_target(
            emitter,
            ctx,
            data,
            &elem_ty,
            Some(&variadic_elem_ty),
        );
        if !boxed_to_mixed {
            abi::emit_incref_if_refcounted(emitter, &elem_ty.codegen_repr());   // retain refcounted tail elements copied into the new variadic array
        }
        match emitter.target.arch {
            crate::codegen::platform::Arch::AArch64 => {
                emitter.instruction(&format!("ldr {}, [sp]", peek_reg));        // reload the variadic array pointer from the stack
            }
            crate::codegen::platform::Arch::X86_64 => {
                emitter.instruction(&format!("mov {}, QWORD PTR [rsp]", peek_reg)); // reload the variadic array pointer from the stack
            }
        }
        match stored_ty {
            PhpType::Int
            | PhpType::Bool
            | PhpType::Resource(_)
            | PhpType::Callable
            | PhpType::Iterable
            | PhpType::Mixed
            | PhpType::Array(_)
            | PhpType::AssocArray { .. }
            | PhpType::Buffer(_)
            | PhpType::Object(_)
            | PhpType::Packed(_)
            | PhpType::Pointer(_)
            | PhpType::Union(_) | PhpType::Never => {
                let dest_reg = len_store_reg;
                emitter.instruction(&format!("mov {}, {}", dest_reg, peek_reg)); // point at the variadic array before skipping the header
                match emitter.target.arch {
                    crate::codegen::platform::Arch::AArch64 => {
                        emitter.instruction(&format!("add {}, {}, #24", dest_reg, dest_reg)); // point at the variadic array payload
                        emitter.instruction(&format!("lsl {}, {}, #3", offset_reg, tail_index_reg)); // compute the 8-byte destination slot offset
                        emitter.instruction(&format!("add {}, {}, {}", dest_reg, dest_reg, offset_reg)); // advance to the selected variadic destination slot
                    }
                    crate::codegen::platform::Arch::X86_64 => {
                        emitter.instruction(&format!("add {}, 24", dest_reg));  // point at the variadic array payload
                        emitter.instruction(&format!("mov {}, {}", offset_reg, tail_index_reg)); // copy the destination index before scaling
                        emitter.instruction(&format!("shl {}, 3", offset_reg)); // compute the 8-byte destination slot offset
                        emitter.instruction(&format!("add {}, {}", dest_reg, offset_reg)); // advance to the selected variadic destination slot
                    }
                }
                abi::emit_store_to_address(emitter, abi::int_result_reg(emitter), dest_reg, 0);
            }
            PhpType::Float => {
                let dest_reg = len_store_reg;
                emitter.instruction(&format!("mov {}, {}", dest_reg, peek_reg)); // point at the variadic array before skipping the header
                match emitter.target.arch {
                    crate::codegen::platform::Arch::AArch64 => {
                        emitter.instruction(&format!("add {}, {}, #24", dest_reg, dest_reg)); // point at the variadic array payload
                        emitter.instruction(&format!("lsl {}, {}, #3", offset_reg, tail_index_reg)); // compute the 8-byte destination slot offset
                        emitter.instruction(&format!("add {}, {}, {}", dest_reg, dest_reg, offset_reg)); // advance to the selected variadic destination slot
                    }
                    crate::codegen::platform::Arch::X86_64 => {
                        emitter.instruction(&format!("add {}, 24", dest_reg));  // point at the variadic array payload
                        emitter.instruction(&format!("mov {}, {}", offset_reg, tail_index_reg)); // copy the destination index before scaling
                        emitter.instruction(&format!("shl {}, 3", offset_reg)); // compute the 8-byte destination slot offset
                        emitter.instruction(&format!("add {}, {}", dest_reg, offset_reg)); // advance to the selected variadic destination slot
                    }
                }
                abi::emit_store_to_address(emitter, abi::float_result_reg(emitter), dest_reg, 0);
            }
            PhpType::Str => {
                let dest_reg = len_store_reg;
                let (ptr_reg, len_reg_out) = abi::string_result_regs(emitter);
                emitter.instruction(&format!("mov {}, {}", dest_reg, peek_reg)); // point at the variadic array before skipping the header
                match emitter.target.arch {
                    crate::codegen::platform::Arch::AArch64 => {
                        emitter.instruction(&format!("add {}, {}, #24", dest_reg, dest_reg)); // point at the variadic array payload
                        emitter.instruction(&format!("lsl {}, {}, #4", offset_reg, tail_index_reg)); // compute the 16-byte destination slot offset
                        emitter.instruction(&format!("add {}, {}, {}", dest_reg, dest_reg, offset_reg)); // advance to the selected variadic destination slot
                    }
                    crate::codegen::platform::Arch::X86_64 => {
                        emitter.instruction(&format!("add {}, 24", dest_reg));  // point at the variadic array payload
                        emitter.instruction(&format!("mov {}, {}", offset_reg, tail_index_reg)); // copy the destination index before scaling
                        emitter.instruction(&format!("shl {}, 4", offset_reg)); // compute the 16-byte destination slot offset
                        emitter.instruction(&format!("add {}, {}", dest_reg, offset_reg)); // advance to the selected variadic destination slot
                    }
                }
                abi::emit_store_to_address(emitter, ptr_reg, dest_reg, 0);
                abi::emit_store_to_address(emitter, len_reg_out, dest_reg, 8);
            }
            PhpType::Void => {}
        }
        match emitter.target.arch {
            crate::codegen::platform::Arch::AArch64 => {
                emitter.instruction(&format!("add {}, {}, #1", tail_index_reg, tail_index_reg)); // advance to the next tail element
            }
            crate::codegen::platform::Arch::X86_64 => {
                emitter.instruction(&format!("add {}, 1", tail_index_reg));     // advance to the next tail element
            }
        }
        abi::emit_store_to_address(emitter, tail_index_reg, peek_reg, 0);       // persist the updated variadic array length
        abi::emit_jump(emitter, &loop_label);
        emitter.label(&loop_done_label);
        emitter.label(&done_label);
        arg_types.push(PhpType::Array(Box::new(variadic_elem_ty)));
    }
    callback_env::push_captures_as_hidden_args(&captures, emitter, ctx, &mut arg_types);

    let assignments = abi::build_outgoing_arg_assignments_for_target(emitter.target, &arg_types, 0);
    let overflow_bytes = abi::materialize_outgoing_args(emitter, &assignments);

    let ret_ty = sig.return_type.clone();

    // -- call callback via the resolved address in x19 --
    if !concat_saved_before_args {
        crate::codegen::expr::save_concat_offset_before_nested_call(emitter, ctx);
    }
    abi::emit_call_reg(emitter, call_reg);
    if concat_saved_before_args {
        abi::emit_release_temporary_stack(emitter, overflow_bytes);
        crate::codegen::expr::restore_concat_offset_after_nested_call(emitter, ctx, &ret_ty);
    } else {
        crate::codegen::expr::restore_concat_offset_after_nested_call(emitter, ctx, &ret_ty);
        abi::emit_release_temporary_stack(emitter, overflow_bytes);
    }

    ret_ty
}

fn emit_loaded_assoc_array_callback_call(
    array_source: LoadedArraySource,
    arr_ty: &PhpType,
    call_reg: &str,
    captures: &[(String, PhpType, bool)],
    sig: &FunctionSig,
    concat_saved_before_args: bool,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> PhpType {
    let hash_reg = match emitter.target.arch {
        Arch::AArch64 => "x20",
        Arch::X86_64 => "r13",
    };
    let elem_ty = match arr_ty {
        PhpType::AssocArray { value, .. } => *value.clone(),
        _ => PhpType::Int,
    };

    match array_source {
        LoadedArraySource::Result => {
            emitter.instruction(&format!("mov {}, {}", hash_reg, abi::int_result_reg(emitter))); // preserve the callback-argument hash pointer across named lookups
        }
        LoadedArraySource::TemporaryStackSlot(offset) => {
            abi::emit_load_temporary_stack_slot(emitter, hash_reg, offset);
        }
    }

    let visible_param_count = sig.params.len();
    let regular_param_count = if sig.variadic.is_some() {
        visible_param_count.saturating_sub(1)
    } else {
        visible_param_count
    };
    let mut arg_types = Vec::new();

    for i in 0..regular_param_count {
        let has_default = sig.defaults.get(i).and_then(|d| d.as_ref()).is_some();
        let target_ty = if args::declared_target_ty(Some(sig), i).is_some() || has_default {
            sig.params.get(i).map(|(_, ty)| ty)
        } else {
            None
        };
        let param_name = sig.params.get(i).map(|(name, _)| name.as_str());
        emitter.comment("lookup call_user_func_array() named argument");
        args::emit_hash_lookup_for_param_or_index(
            hash_reg,
            param_name,
            i,
            emitter,
            ctx,
            data,
        );

        let pushed_ty = if let Some(default_expr) = sig.defaults.get(i).and_then(|d| d.as_ref()) {
            let use_default = ctx.next_label("cufa_assoc_default");
            let done = ctx.next_label("cufa_assoc_done");
            abi::emit_branch_if_int_result_zero(emitter, &use_default);
            let loaded_ty = args::push_loaded_hash_value_arg(&elem_ty, target_ty, emitter, ctx, data);
            abi::emit_jump(emitter, &done);
            emitter.label(&use_default);
            let default_ty = args::push_expr_arg(default_expr, target_ty, emitter, ctx, data);
            emitter.label(&done);
            widen_callback_arg_type(&loaded_ty, &default_ty)
        } else {
            let missing = ctx.next_label("cufa_assoc_missing");
            let done = ctx.next_label("cufa_assoc_done");
            abi::emit_branch_if_int_result_zero(emitter, &missing);
            let loaded_ty = args::push_loaded_hash_value_arg(&elem_ty, target_ty, emitter, ctx, data);
            abi::emit_jump(emitter, &done);
            emitter.label(&missing);
            emit_call_user_func_array_missing_arg_abort(emitter, data);
            emitter.label(&done);
            loaded_ty
        };
        arg_types.push(pushed_ty);
    }

    if sig.variadic.is_some() {
        emitter.comment("empty variadic array for associative call_user_func_array()");
        let variadic_ty = args::emit_empty_variadic_array_arg(
            "empty variadic array for associative call_user_func_array()",
            emitter,
        );
        arg_types.push(variadic_ty);
    }

    callback_env::push_captures_as_hidden_args(captures, emitter, ctx, &mut arg_types);
    let assignments = abi::build_outgoing_arg_assignments_for_target(emitter.target, &arg_types, 0);
    let overflow_bytes = abi::materialize_outgoing_args(emitter, &assignments);
    let ret_ty = sig.return_type.clone();

    if !concat_saved_before_args {
        crate::codegen::expr::save_concat_offset_before_nested_call(emitter, ctx);
    }
    abi::emit_call_reg(emitter, call_reg);
    if concat_saved_before_args {
        abi::emit_release_temporary_stack(emitter, overflow_bytes);
        crate::codegen::expr::restore_concat_offset_after_nested_call(emitter, ctx, &ret_ty);
    } else {
        crate::codegen::expr::restore_concat_offset_after_nested_call(emitter, ctx, &ret_ty);
        abi::emit_release_temporary_stack(emitter, overflow_bytes);
    }

    ret_ty
}

pub(crate) fn emit_loaded_array_unknown_callback_call(
    array_source: LoadedArraySource,
    arr_ty: &PhpType,
    call_reg: &str,
    captures: &[(String, PhpType, bool)],
    concat_saved_before_args: bool,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> PhpType {
    if captures.is_empty() {
        return emit_loaded_array_unknown_callback_call_dynamic(
            array_source,
            arr_ty,
            call_reg,
            concat_saved_before_args,
            emitter,
            ctx,
        );
    }

    let (array_reg, len_reg) = match emitter.target.arch {
        Arch::AArch64 => ("x20", "x21"),
        Arch::X86_64 => ("r13", "r14"),
    };
    let elem_ty = match arr_ty {
        PhpType::Array(elem_ty) => *elem_ty.clone(),
        _ => PhpType::Int,
    };
    let elem_size = args::array_element_stride(&elem_ty);

    match array_source {
        LoadedArraySource::Result => {
            emitter.instruction(&format!("mov {}, {}", array_reg, abi::int_result_reg(emitter))); // preserve the callback-argument array pointer for unknown signature dispatch
        }
        LoadedArraySource::TemporaryStackSlot(offset) => {
            abi::emit_load_temporary_stack_slot(emitter, array_reg, offset);
        }
    }
    abi::emit_load_from_address(emitter, len_reg, array_reg, 0);                // load callback-argument array length for unknown signature dispatch

    let done_label = ctx.next_label("cufa_unknown_done");
    let case_labels: Vec<String> = (0..=UNKNOWN_CALLBACK_ARG_LIMIT)
        .map(|_| ctx.next_label("cufa_unknown_arity"))
        .collect();
    for (arg_count, label) in case_labels.iter().enumerate() {
        match emitter.target.arch {
            Arch::AArch64 => {
                emitter.instruction(&format!("cmp {}, #{}", len_reg, arg_count)); // compare runtime callback-argument count against this unknown-signature case
                emitter.instruction(&format!("b.eq {}", label));                // dispatch to the call shape matching the runtime argument count
            }
            Arch::X86_64 => {
                emitter.instruction(&format!("cmp {}, {}", len_reg, arg_count)); // compare runtime callback-argument count against this unknown-signature case
                emitter.instruction(&format!("je {}", label));                  // dispatch to the call shape matching the runtime argument count
            }
        }
    }
    emit_unknown_callback_arg_limit_abort(emitter, data);

    for (arg_count, label) in case_labels.iter().enumerate() {
        emitter.label(label);
        emit_unknown_callback_case(
            arg_count,
            &elem_ty,
            elem_size,
            array_reg,
            call_reg,
            captures,
            concat_saved_before_args,
            &done_label,
            emitter,
            ctx,
            data,
        );
    }

    emitter.label(&done_label);
    PhpType::Int
}

fn unknown_callback_register_arg_capacity(target: crate::codegen::platform::Target, elem_ty: &PhpType) -> usize {
    match elem_ty.codegen_repr() {
        PhpType::Float => 8,
        PhpType::Str => match target.arch {
            Arch::AArch64 => 4,
            Arch::X86_64 => 3,
        },
        PhpType::Void | PhpType::Never => 0,
        _ => match target.arch {
            Arch::AArch64 => 8,
            Arch::X86_64 => 6,
        },
    }
}

fn emit_loaded_array_unknown_callback_call_dynamic(
    array_source: LoadedArraySource,
    arr_ty: &PhpType,
    call_reg: &str,
    concat_saved_before_args: bool,
    emitter: &mut Emitter,
    ctx: &mut Context,
) -> PhpType {
    let (array_reg, len_reg, overflow_count_reg, overflow_bytes_reg) = match emitter.target.arch {
        Arch::AArch64 => ("x20", "x21", "x22", "x23"),
        Arch::X86_64 => ("r13", "r14", "r15", "rbx"),
    };
    let elem_ty = match arr_ty {
        PhpType::Array(elem_ty) => *elem_ty.clone(),
        _ => PhpType::Int,
    };
    let register_arg_capacity = unknown_callback_register_arg_capacity(emitter.target, &elem_ty);
    let register_temp_bytes = register_arg_capacity * 16;

    match array_source {
        LoadedArraySource::Result => {
            emitter.instruction(&format!("mov {}, {}", array_reg, abi::int_result_reg(emitter))); // preserve the callback-argument array pointer for dynamic unknown-signature dispatch
        }
        LoadedArraySource::TemporaryStackSlot(offset) => {
            abi::emit_load_temporary_stack_slot(emitter, array_reg, offset);
        }
    }
    abi::emit_load_from_address(emitter, len_reg, array_reg, 0);                // load the dynamic callback-argument count
    emit_unknown_dynamic_overflow_size(
        len_reg,
        overflow_count_reg,
        overflow_bytes_reg,
        register_arg_capacity,
        emitter,
        ctx,
    );
    emit_dynamic_stack_adjust(emitter, overflow_bytes_reg, true);
    abi::emit_reserve_temporary_stack(emitter, register_temp_bytes);

    emit_unknown_dynamic_register_arg_temps(
        array_reg,
        len_reg,
        &elem_ty,
        register_arg_capacity,
        emitter,
        ctx,
    );
    emit_unknown_dynamic_stack_args(
        array_reg,
        overflow_count_reg,
        &elem_ty,
        register_arg_capacity,
        register_temp_bytes,
        emitter,
        ctx,
    );
    emit_unknown_dynamic_load_register_args(
        len_reg,
        &elem_ty,
        register_arg_capacity,
        emitter,
        ctx,
    );

    abi::emit_release_temporary_stack(emitter, register_temp_bytes);
    let ret_ty = PhpType::Int;
    if !concat_saved_before_args {
        crate::codegen::expr::save_concat_offset_before_nested_call(emitter, ctx);
    }
    abi::emit_call_reg(emitter, call_reg);
    if concat_saved_before_args {
        emit_dynamic_stack_adjust(emitter, overflow_bytes_reg, false);
        crate::codegen::expr::restore_concat_offset_after_nested_call(emitter, ctx, &ret_ty);
    } else {
        crate::codegen::expr::restore_concat_offset_after_nested_call(emitter, ctx, &ret_ty);
        emit_dynamic_stack_adjust(emitter, overflow_bytes_reg, false);
    }

    ret_ty
}

fn emit_unknown_dynamic_overflow_size(
    len_reg: &str,
    overflow_count_reg: &str,
    overflow_bytes_reg: &str,
    register_arg_capacity: usize,
    emitter: &mut Emitter,
    ctx: &mut Context,
) {
    let has_overflow = ctx.next_label("cufa_unknown_dynamic_overflow");
    let done = ctx.next_label("cufa_unknown_dynamic_overflow_done");
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction(&format!("mov {}, #0", overflow_count_reg));    // default to no stack-passed unknown callback arguments
            emitter.instruction(&format!("cmp {}, #{}", len_reg, register_arg_capacity)); // compare runtime arity with the register argument capacity
            emitter.instruction(&format!("b.gt {}", has_overflow));             // compute stack spill bytes only when runtime arity exceeds registers
            emitter.instruction(&format!("b {}", done));                        // skip overflow sizing for register-only calls
            emitter.label(&has_overflow);
            emitter.instruction(&format!("sub {}, {}, #{}", overflow_count_reg, len_reg, register_arg_capacity)); // count callback arguments that must be stack-passed
            emitter.label(&done);
            emitter.instruction(&format!("lsl {}, {}, #4", overflow_bytes_reg, overflow_count_reg)); // convert stack-passed argument count to 16-byte ABI slots
        }
        Arch::X86_64 => {
            emitter.instruction(&format!("mov {}, 0", overflow_count_reg));     // default to no stack-passed unknown callback arguments
            emitter.instruction(&format!("cmp {}, {}", len_reg, register_arg_capacity)); // compare runtime arity with the register argument capacity
            emitter.instruction(&format!("jg {}", has_overflow));               // compute stack spill bytes only when runtime arity exceeds registers
            emitter.instruction(&format!("jmp {}", done));                      // skip overflow sizing for register-only calls
            emitter.label(&has_overflow);
            emitter.instruction(&format!("mov {}, {}", overflow_count_reg, len_reg)); // seed overflow count from the runtime callback arity
            emitter.instruction(&format!("sub {}, {}", overflow_count_reg, register_arg_capacity)); // count callback arguments that must be stack-passed
            emitter.label(&done);
            emitter.instruction(&format!("mov {}, {}", overflow_bytes_reg, overflow_count_reg)); // copy overflow count before scaling to bytes
            emitter.instruction(&format!("shl {}, 4", overflow_bytes_reg));     // convert stack-passed argument count to 16-byte ABI slots
        }
    }
}

fn emit_dynamic_stack_adjust(emitter: &mut Emitter, bytes_reg: &str, subtract: bool) {
    match (emitter.target.arch, subtract) {
        (Arch::AArch64, true) => {
            emitter.instruction(&format!("sub sp, sp, {}", bytes_reg));         // reserve dynamic stack space for unknown callback overflow arguments
        }
        (Arch::AArch64, false) => {
            emitter.instruction(&format!("add sp, sp, {}", bytes_reg));         // release dynamic stack space for unknown callback overflow arguments
        }
        (Arch::X86_64, true) => {
            emitter.instruction(&format!("sub rsp, {}", bytes_reg));            // reserve dynamic stack space for unknown callback overflow arguments
        }
        (Arch::X86_64, false) => {
            emitter.instruction(&format!("add rsp, {}", bytes_reg));            // release dynamic stack space for unknown callback overflow arguments
        }
    }
}

fn emit_unknown_dynamic_register_arg_temps(
    array_reg: &str,
    len_reg: &str,
    elem_ty: &PhpType,
    register_arg_capacity: usize,
    emitter: &mut Emitter,
    ctx: &mut Context,
) {
    for arg_idx in 0..register_arg_capacity {
        let load_label = ctx.next_label("cufa_unknown_dynamic_reg_load");
        let done_label = ctx.next_label("cufa_unknown_dynamic_reg_done");
        match emitter.target.arch {
            Arch::AArch64 => {
                emitter.instruction(&format!("cmp {}, #{}", len_reg, arg_idx + 1)); // check whether this register-passed callback argument exists
                emitter.instruction(&format!("b.ge {}", load_label));           // materialize the register argument when present
                emitter.instruction(&format!("b {}", done_label));              // leave absent optional unknown callback argument registers untouched
            }
            Arch::X86_64 => {
                emitter.instruction(&format!("cmp {}, {}", len_reg, arg_idx + 1)); // check whether this register-passed callback argument exists
                emitter.instruction(&format!("jge {}", load_label));            // materialize the register argument when present
                emitter.instruction(&format!("jmp {}", done_label));            // leave absent optional unknown callback argument registers untouched
            }
        }
        emitter.label(&load_label);
        args::load_array_element_to_result(
            emitter,
            elem_ty,
            array_reg,
            24 + arg_idx * args::array_element_stride(elem_ty),
        );
        abi::emit_incref_if_refcounted(emitter, &elem_ty.codegen_repr());       // retain borrowed heap arguments before passing them to the unknown callback
        emit_store_current_result_to_sp_offset(emitter, elem_ty, arg_idx * 16);
        emitter.label(&done_label);
    }
}

fn emit_unknown_dynamic_stack_args(
    array_reg: &str,
    overflow_count_reg: &str,
    elem_ty: &PhpType,
    register_arg_capacity: usize,
    register_temp_bytes: usize,
    emitter: &mut Emitter,
    ctx: &mut Context,
) {
    let loop_label = ctx.next_label("cufa_unknown_dynamic_stack_loop");
    let done_label = ctx.next_label("cufa_unknown_dynamic_stack_done");
    let (idx_reg, source_idx_reg, source_reg, dest_reg, offset_reg) = match emitter.target.arch {
        Arch::AArch64 => ("x24", "x25", "x26", "x27", "x28"),
        Arch::X86_64 => ("rcx", "r10", "r11", "rsi", "rdx"),
    };
    abi::emit_load_int_immediate(emitter, idx_reg, 0);
    emitter.label(&loop_label);
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction(&format!("cmp {}, {}", idx_reg, overflow_count_reg)); // stop after all stack-passed unknown callback args are materialized
            emitter.instruction(&format!("b.ge {}", done_label));               // leave the overflow materialization loop
            emitter.instruction(&format!("add {}, {}, #{}", source_idx_reg, idx_reg, register_arg_capacity)); // convert overflow index to source array index
        }
        Arch::X86_64 => {
            emitter.instruction(&format!("cmp {}, {}", idx_reg, overflow_count_reg)); // stop after all stack-passed unknown callback args are materialized
            emitter.instruction(&format!("jge {}", done_label));                // leave the overflow materialization loop
            emitter.instruction(&format!("mov {}, {}", source_idx_reg, idx_reg)); // seed source array index from overflow index
            emitter.instruction(&format!("add {}, {}", source_idx_reg, register_arg_capacity)); // convert overflow index to source array index
        }
    }
    emit_dynamic_array_element_to_result(
        array_reg,
        source_idx_reg,
        source_reg,
        offset_reg,
        elem_ty,
        emitter,
    );
    abi::emit_incref_if_refcounted(emitter, &elem_ty.codegen_repr());           // retain borrowed heap overflow arguments before passing them to the unknown callback
    emit_unknown_dynamic_stack_arg_address(
        idx_reg,
        dest_reg,
        offset_reg,
        register_temp_bytes,
        emitter,
    );
    emit_store_current_result_to_address(emitter, elem_ty, dest_reg);
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction(&format!("add {}, {}, #1", idx_reg, idx_reg));  // advance to the next stack-passed unknown callback argument
            emitter.instruction(&format!("b {}", loop_label));                  // continue materializing overflow arguments
        }
        Arch::X86_64 => {
            emitter.instruction(&format!("add {}, 1", idx_reg));                // advance to the next stack-passed unknown callback argument
            emitter.instruction(&format!("jmp {}", loop_label));                // continue materializing overflow arguments
        }
    }
    emitter.label(&done_label);
}

fn emit_dynamic_array_element_to_result(
    array_reg: &str,
    index_reg: &str,
    source_reg: &str,
    offset_reg: &str,
    elem_ty: &PhpType,
    emitter: &mut Emitter,
) {
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction(&format!("mov {}, {}", source_reg, array_reg)); // seed the dynamic source element pointer from the callback-argument array
            emitter.instruction(&format!("add {}, {}, #24", source_reg, source_reg)); // skip the indexed array header before dynamic argument lookup
            if args::array_element_stride(elem_ty) == 16 {
                emitter.instruction(&format!("lsl {}, {}, #4", offset_reg, index_reg)); // scale dynamic source index by the string element width
            } else {
                emitter.instruction(&format!("lsl {}, {}, #3", offset_reg, index_reg)); // scale dynamic source index by the scalar element width
            }
            emitter.instruction(&format!("add {}, {}, {}", source_reg, source_reg, offset_reg)); // address the selected dynamic callback argument slot
        }
        Arch::X86_64 => {
            emitter.instruction(&format!("mov {}, {}", source_reg, array_reg)); // seed the dynamic source element pointer from the callback-argument array
            emitter.instruction(&format!("add {}, 24", source_reg));            // skip the indexed array header before dynamic argument lookup
            emitter.instruction(&format!("mov {}, {}", offset_reg, index_reg)); // copy dynamic source index before scaling
            emitter.instruction(&format!("imul {}, {}", offset_reg, args::array_element_stride(elem_ty))); // scale dynamic source index by the element width
            emitter.instruction(&format!("add {}, {}", source_reg, offset_reg)); // address the selected dynamic callback argument slot
        }
    }
    args::load_array_element_to_result(emitter, elem_ty, source_reg, 0);
}

fn emit_unknown_dynamic_stack_arg_address(
    idx_reg: &str,
    dest_reg: &str,
    offset_reg: &str,
    register_temp_bytes: usize,
    emitter: &mut Emitter,
) {
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction(&format!("mov {}, sp", dest_reg));              // seed overflow destination from the current stack pointer
            emitter.instruction(&format!("add {}, {}, #{}", dest_reg, dest_reg, register_temp_bytes)); // skip register-argument temp slots to reach outgoing stack args
            emitter.instruction(&format!("lsl {}, {}, #4", offset_reg, idx_reg)); // scale overflow argument index by the 16-byte ABI slot width
            emitter.instruction(&format!("add {}, {}, {}", dest_reg, dest_reg, offset_reg)); // address the outgoing overflow argument slot
        }
        Arch::X86_64 => {
            emitter.instruction(&format!("lea {}, [rsp + {}]", dest_reg, register_temp_bytes)); // address the first outgoing overflow argument slot
            emitter.instruction(&format!("mov {}, {}", offset_reg, idx_reg));   // copy overflow argument index before scaling to bytes
            emitter.instruction(&format!("shl {}, 4", offset_reg));             // scale overflow argument index by the 16-byte ABI slot width
            emitter.instruction(&format!("add {}, {}", dest_reg, offset_reg));  // address the outgoing overflow argument slot
        }
    }
}

fn emit_unknown_dynamic_load_register_args(
    len_reg: &str,
    elem_ty: &PhpType,
    register_arg_capacity: usize,
    emitter: &mut Emitter,
    ctx: &mut Context,
) {
    for arg_idx in 0..register_arg_capacity {
        let load_label = ctx.next_label("cufa_unknown_dynamic_arg_load");
        let done_label = ctx.next_label("cufa_unknown_dynamic_arg_done");
        match emitter.target.arch {
            Arch::AArch64 => {
                emitter.instruction(&format!("cmp {}, #{}", len_reg, arg_idx + 1)); // check whether this final register argument exists
                emitter.instruction(&format!("b.ge {}", load_label));           // load the ABI register when the argument was provided
                emitter.instruction(&format!("b {}", done_label));              // skip absent unknown callback argument registers
            }
            Arch::X86_64 => {
                emitter.instruction(&format!("cmp {}, {}", len_reg, arg_idx + 1)); // check whether this final register argument exists
                emitter.instruction(&format!("jge {}", load_label));            // load the ABI register when the argument was provided
                emitter.instruction(&format!("jmp {}", done_label));            // skip absent unknown callback argument registers
            }
        }
        emitter.label(&load_label);
        emit_load_sp_offset_to_arg_register(emitter, elem_ty, arg_idx * 16, arg_idx);
        emitter.label(&done_label);
    }
}

fn emit_store_current_result_to_sp_offset(emitter: &mut Emitter, ty: &PhpType, offset: usize) {
    let stack_reg = match emitter.target.arch {
        Arch::AArch64 => "sp",
        Arch::X86_64 => "rsp",
    };
    emit_store_current_result_to_address_offset(emitter, ty, stack_reg, offset);
}

fn emit_store_current_result_to_address(emitter: &mut Emitter, ty: &PhpType, address_reg: &str) {
    emit_store_current_result_to_address_offset(emitter, ty, address_reg, 0);
}

fn emit_store_current_result_to_address_offset(
    emitter: &mut Emitter,
    ty: &PhpType,
    address_reg: &str,
    offset: usize,
) {
    match ty.codegen_repr() {
        PhpType::Float => {
            abi::emit_store_to_address(emitter, abi::float_result_reg(emitter), address_reg, offset);
        }
        PhpType::Str => {
            let (ptr_reg, len_reg) = abi::string_result_regs(emitter);
            abi::emit_store_to_address(emitter, ptr_reg, address_reg, offset);
            abi::emit_store_to_address(emitter, len_reg, address_reg, offset + 8);
        }
        PhpType::Void | PhpType::Never => {}
        _ => {
            abi::emit_store_to_address(emitter, abi::int_result_reg(emitter), address_reg, offset);
        }
    }
}

fn emit_load_sp_offset_to_arg_register(
    emitter: &mut Emitter,
    ty: &PhpType,
    offset: usize,
    arg_idx: usize,
) {
    match ty.codegen_repr() {
        PhpType::Float => {
            let reg = abi::float_arg_reg_name(emitter.target, arg_idx);
            abi::emit_load_temporary_stack_slot(emitter, reg, offset);
        }
        PhpType::Str => {
            let ptr_reg = abi::int_arg_reg_name(emitter.target, arg_idx * 2);
            let len_reg = abi::int_arg_reg_name(emitter.target, arg_idx * 2 + 1);
            abi::emit_load_temporary_stack_slot(emitter, ptr_reg, offset);
            abi::emit_load_temporary_stack_slot(emitter, len_reg, offset + 8);
        }
        PhpType::Void | PhpType::Never => {}
        _ => {
            let reg = abi::int_arg_reg_name(emitter.target, arg_idx);
            abi::emit_load_temporary_stack_slot(emitter, reg, offset);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_unknown_callback_case(
    arg_count: usize,
    elem_ty: &PhpType,
    elem_size: usize,
    array_reg: &str,
    call_reg: &str,
    captures: &[(String, PhpType, bool)],
    concat_saved_before_args: bool,
    done_label: &str,
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) {
    let mut arg_types = Vec::with_capacity(arg_count + captures.len());
    for i in 0..arg_count {
        args::load_array_element_to_result(emitter, elem_ty, array_reg, 24 + i * elem_size);
        let pushed_ty = args::push_loaded_array_element_arg(elem_ty, None, emitter, ctx, data);
        arg_types.push(pushed_ty);
    }
    callback_env::push_captures_as_hidden_args(captures, emitter, ctx, &mut arg_types);

    let assignments = abi::build_outgoing_arg_assignments_for_target(emitter.target, &arg_types, 0);
    let overflow_bytes = abi::materialize_outgoing_args(emitter, &assignments);
    let ret_ty = PhpType::Int;

    if !concat_saved_before_args {
        crate::codegen::expr::save_concat_offset_before_nested_call(emitter, ctx);
    }
    abi::emit_call_reg(emitter, call_reg);
    if concat_saved_before_args {
        abi::emit_release_temporary_stack(emitter, overflow_bytes);
        crate::codegen::expr::restore_concat_offset_after_nested_call(emitter, ctx, &ret_ty);
    } else {
        crate::codegen::expr::restore_concat_offset_after_nested_call(emitter, ctx, &ret_ty);
        abi::emit_release_temporary_stack(emitter, overflow_bytes);
    }
    abi::emit_jump(emitter, done_label);
}

fn emit_unknown_callback_arg_limit_abort(emitter: &mut Emitter, data: &mut DataSection) {
    let (message_label, message_len) = data.add_string(
        b"Fatal error: call_user_func_array() unknown-signature callback argument array is too large\n",
    );
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction("mov x0, #2");                                  // write the unknown-signature arity diagnostic to stderr
            emitter.adrp("x1", &message_label);
            emitter.add_lo12("x1", "x1", &message_label);
            emitter.instruction(&format!("mov x2, #{}", message_len));          // pass the diagnostic byte length to write()
            emitter.syscall(4);
            abi::emit_exit(emitter, 1);
        }
        Arch::X86_64 => {
            emitter.instruction("mov edi, 2");                                  // write the unknown-signature arity diagnostic to stderr
            abi::emit_symbol_address(emitter, "rsi", &message_label);
            emitter.instruction(&format!("mov edx, {}", message_len));          // pass the diagnostic byte length to write()
            emitter.instruction("mov eax, 1");                                  // Linux x86_64 syscall 1 = write
            emitter.instruction("syscall");                                     // emit the fatal unknown-signature arity diagnostic
            abi::emit_exit(emitter, 1);
        }
    }
}

fn emit_call_user_func_array_missing_arg_abort(emitter: &mut Emitter, data: &mut DataSection) {
    let (message_label, message_len) = data.add_string(
        b"Fatal error: call_user_func_array() argument array is missing a required callback parameter\n",
    );
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction("mov x0, #2");                                  // write the callback argument diagnostic to stderr
            emitter.adrp("x1", &message_label);
            emitter.add_lo12("x1", "x1", &message_label);
            emitter.instruction(&format!("mov x2, #{}", message_len));          // pass the diagnostic byte length to write()
            emitter.syscall(4);
            abi::emit_exit(emitter, 1);
        }
        Arch::X86_64 => {
            emitter.instruction("mov edi, 2");                                  // write the callback argument diagnostic to stderr
            abi::emit_symbol_address(emitter, "rsi", &message_label);
            emitter.instruction(&format!("mov edx, {}", message_len));          // pass the diagnostic byte length to write()
            emitter.instruction("mov eax, 1");                                  // Linux x86_64 syscall 1 = write
            emitter.instruction("syscall");                                     // emit the fatal callback argument diagnostic
            abi::emit_exit(emitter, 1);
        }
    }
}

fn widen_callback_arg_type(a: &PhpType, b: &PhpType) -> PhpType {
    if a == b {
        return a.clone();
    }
    if matches!(a, PhpType::Mixed | PhpType::Union(_))
        || matches!(b, PhpType::Mixed | PhpType::Union(_))
    {
        return PhpType::Mixed;
    }
    if *a == PhpType::Str || *b == PhpType::Str {
        return PhpType::Str;
    }
    if *a == PhpType::Float || *b == PhpType::Float {
        return PhpType::Float;
    }
    if *a == PhpType::Void {
        return b.clone();
    }
    if *b == PhpType::Void {
        return a.clone();
    }
    a.clone()
}
