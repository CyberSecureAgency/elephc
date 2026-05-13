//! Purpose:
//! Emits PHP `json_encode` JSON builtin calls.
//! Marshals PHP scalar, array, and Mixed values into runtime JSON helpers and error state.
//!
//! Called from:
//! - `crate::codegen::builtins::system::emit()`.
//!
//! Key details:
//! - JSON error state is runtime-global observable state and must stay coupled to json_last_error().

use crate::codegen::abi;
use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::expr::emit_expr;
use crate::parser::ast::Expr;
use crate::types::PhpType;

pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("json_encode()");

    let ty = emit_expr(&args[0], emitter, ctx, data);
    persist_string_result_if_needed(&ty, emitter);
    abi::emit_push_result_value(emitter, &ty);

    if let Some(flag_expr) = args.get(1) {
        emit_expr(flag_expr, emitter, ctx, data);
        abi::emit_push_reg(emitter, abi::int_result_reg(emitter));              // keep json_encode flags stable while later arguments evaluate
    }
    if let Some(depth_expr) = args.get(2) {
        emit_expr(depth_expr, emitter, ctx, data);
        abi::emit_push_reg(emitter, abi::int_result_reg(emitter));              // keep json_encode depth stable until all argument side effects are done
    }

    // PHP evaluates every argument before the builtin mutates global JSON
    // error/configuration state.
    abi::emit_store_zero_to_symbol(emitter, "_json_last_error", 0);
    abi::emit_store_zero_to_symbol(emitter, "_json_active_depth", 0);

    if args.get(2).is_some() {
        abi::emit_pop_reg(emitter, abi::int_result_reg(emitter));
        abi::emit_store_reg_to_symbol(
            emitter,
            abi::int_result_reg(emitter),
            "_json_depth_limit",
            0,
        );
    } else {
        abi::emit_load_int_immediate(emitter, abi::int_result_reg(emitter), 512);
        abi::emit_store_reg_to_symbol(
            emitter,
            abi::int_result_reg(emitter),
            "_json_depth_limit",
            0,
        );
    }
    if args.get(1).is_some() {
        abi::emit_pop_reg(emitter, abi::int_result_reg(emitter));
        abi::emit_store_reg_to_symbol(
            emitter,
            abi::int_result_reg(emitter),
            "_json_active_flags",
            0,
        );
    } else {
        abi::emit_store_zero_to_symbol(emitter, "_json_active_flags", 0);
    }

    restore_result_value(emitter, &ty);

    match ty {
        PhpType::Int => {
            // -- convert integer to JSON (just itoa) --
            abi::emit_call_label(emitter, "__rt_itoa");                         // convert the integer payload into a JSON decimal string for the active target ABI
        }
        PhpType::Float => {
            // -- convert float to JSON, rejecting Inf/NaN --
            abi::emit_call_label(emitter, "__rt_json_encode_float");            // detect Inf/NaN, set JSON_ERROR_INF_OR_NAN, throw if requested, then encode
        }
        PhpType::Bool => {
            // -- convert bool to JSON "true"/"false" --
            abi::emit_call_label(emitter, "__rt_json_encode_bool");             // convert the bool payload into the JSON literals true/false for the active target ABI
        }
        PhpType::Str => {
            // -- wrap string with JSON quotes and escape special chars --
            abi::emit_call_label(emitter, "__rt_json_encode_str");              // escape and quote the string payload into JSON using the active target ABI
        }
        PhpType::Void => {
            // -- null → "null" --
            abi::emit_call_label(emitter, "__rt_json_encode_null");             // produce the JSON null literal using the active target ABI
        }
        PhpType::Array(ref elem_ty) => {
            match elem_ty.as_ref() {
                PhpType::Int => {
                    // x0 = array pointer
                    abi::emit_call_label(emitter, "__rt_json_encode_array_int"); // encode an integer array to JSON using the active target ABI
                }
                PhpType::Str => {
                    // x0 = array pointer
                    abi::emit_call_label(emitter, "__rt_json_encode_array_str"); // encode a string array to JSON using the active target ABI
                }
                _ => {
                    // Fallback: inspect the packed runtime value_type tag per array
                    abi::emit_call_label(emitter, "__rt_json_encode_array_dynamic"); // encode the array to JSON by inspecting its runtime value_type tag
                }
            }
        }
        PhpType::AssocArray { .. } => {
            // x0 = hash table pointer
            abi::emit_call_label(emitter, "__rt_json_encode_assoc");            // encode the associative array to JSON using the active target ABI
        }
        PhpType::Object(class_name) => {
            if crate::types::checker::builtin_stdclass::is_stdclass(&class_name) {
                // stdClass has no static descriptor; encode the dynamic
                // property hash through the assoc-array encoder.
                match emitter.target.arch {
                    crate::codegen::platform::Arch::AArch64 => {
                        emitter.instruction("ldr x0, [x0, #8]");                // load the dynamic-property hash from obj+8
                    }
                    crate::codegen::platform::Arch::X86_64 => {
                        emitter.instruction("mov rax, QWORD PTR [rax + 8]");    // load the dynamic-property hash from obj+8
                    }
                }
                abi::emit_call_label(emitter, "__rt_json_encode_stdclass");     // encode the hash through the stdClass-aware encoder (empty hash → `{}`)
            } else {
                // x0 = object pointer; dispatches to JsonSerializable when present.
                abi::emit_call_label(emitter, "__rt_json_encode_object");       // encode the object via the per-class JSON descriptor walker
            }
        }
        PhpType::Mixed => {
            // x0 = boxed mixed pointer
            abi::emit_call_label(emitter, "__rt_json_encode_mixed");            // inspect the boxed payload and encode it as JSON for the active target ABI
        }
        _ => {
            // Fallback: encode as "null"
            abi::emit_call_label(emitter, "__rt_json_encode_null");             // produce the JSON null literal for unsupported payloads
        }
    }

    // Apply post-process flags (currently JSON_PRETTY_PRINT). The runtime
    // helper is a no-op when the flag bit is clear, so this stays cheap for
    // the common compact encoding path.
    abi::emit_call_label(emitter, "__rt_json_pretty_apply");

    Some(PhpType::Str)
}

fn persist_string_result_if_needed(ty: &PhpType, emitter: &mut Emitter) {
    if ty.codegen_repr() == PhpType::Str {
        abi::emit_call_label(emitter, "__rt_str_persist");                      // keep the string value stable while later json_encode arguments evaluate
    }
}

fn restore_result_value(emitter: &mut Emitter, ty: &PhpType) {
    match ty.codegen_repr() {
        PhpType::Float => {
            abi::emit_pop_float_reg(emitter, abi::float_result_reg(emitter));
        }
        PhpType::Str => {
            let (ptr_reg, len_reg) = abi::string_result_regs(emitter);
            abi::emit_pop_reg_pair(emitter, ptr_reg, len_reg);
        }
        PhpType::Void | PhpType::Never => {}
        _ => {
            abi::emit_pop_reg(emitter, abi::int_result_reg(emitter));
        }
    }
}
