use crate::codegen::abi;
use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::expr::emit_expr;
use crate::codegen::platform::Arch;
use crate::parser::ast::{Expr, ExprKind};
use crate::types::PhpType;

pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("json_decode()");

    // PHP resets json_last_error() at the start of every call so a previous
    // failure does not leak into the next one's success result.
    abi::emit_store_zero_to_symbol(emitter, "_json_last_error", 0);

    // The runtime decoder consults `_json_decode_assoc` whenever it boxes a
    // JSON object: 0 produces a stdClass instance (PHP's default), 1
    // produces a hash-backed associative array. Settle this once at the top
    // level so all recursive object decodes inside one call see the same
    // shape.
    write_assoc_flag(args, emitter, ctx, data);

    // Settle the flag and depth runtime symbols before calling
    // __rt_json_validate so JSON_THROW_ON_ERROR (and the depth limit)
    // observe what the caller passed. The third positional argument is
    // depth and the fourth is flags; both default to PHP's standard
    // values when omitted.
    if let Some(flag_expr) = args.get(3) {
        emit_expr(flag_expr, emitter, ctx, data);
        abi::emit_store_reg_to_symbol(
            emitter,
            abi::int_result_reg(emitter),
            "_json_active_flags",
            0,
        );
    } else {
        abi::emit_store_zero_to_symbol(emitter, "_json_active_flags", 0);
    }
    abi::emit_store_zero_to_symbol(emitter, "_json_active_depth", 0);
    // PHP json_decode rejects nesting when active_depth >= depth (strict).
    // The shared __rt_json_depth_enter compares `active <= limit` so we
    // subtract 1 from the user-supplied depth here to get the same
    // observable behavior (depth=1 → limit=0 → top-level container fails).
    if let Some(depth_expr) = args.get(2) {
        emit_expr(depth_expr, emitter, ctx, data);
        let reg = abi::int_result_reg(emitter);
        match emitter.target.arch {
            Arch::AArch64 => emitter.instruction(&format!("sub {reg}, {reg}, #1")), // strict-semantic offset for json_decode
            Arch::X86_64 => emitter.instruction(&format!("sub {reg}, 1")),         // strict-semantic offset for json_decode
        }
        abi::emit_store_reg_to_symbol(emitter, reg, "_json_depth_limit", 0);
    } else {
        abi::emit_load_int_immediate(emitter, abi::int_result_reg(emitter), 511);
        abi::emit_store_reg_to_symbol(
            emitter,
            abi::int_result_reg(emitter),
            "_json_depth_limit",
            0,
        );
    }

    // -- evaluate the JSON string argument once and stash for the two
    //    runtime calls (validate then decode). emit_expr is not idempotent
    //    when the source has side effects, so we cannot re-evaluate the
    //    argument across two helper calls.
    emit_expr(&args[0], emitter, ctx, data);
    let valid_label = ctx.next_label("json_decode_valid");
    let done_label = ctx.next_label("json_decode_done");
    match emitter.target.arch {
        Arch::AArch64 => {
            // x1 = string ptr, x2 = string len after emit_expr.
            emitter.instruction("stp x1, x2, [sp, #-16]!");                    // park the json source slice across the validator call
            emitter.instruction("bl __rt_json_validate");                      // RFC 8259 validator; returns 1 on success, 0 on failure (and sets _json_last_error)
            emitter.instruction("ldp x1, x2, [sp], #16");                      // restore the json source slice for the structural decoder
            emitter.instruction(&format!("cbnz x0, {}", valid_label));          // valid input → fall through to structural decode
            // Invalid: return Mixed(null) without invoking the decoder.
            emitter.instruction("mov x0, #8");                                 // tag = 8 (null)
            emitter.instruction("mov x1, #0");                                 // value_lo = 0
            emitter.instruction("mov x2, #0");                                 // value_hi = 0
            emitter.instruction("bl __rt_mixed_from_value");                   // box Mixed(null) so callers see a uniform result shape
            emitter.instruction(&format!("b {}", done_label));                  // skip the structural decoder when validation already rejected the input
            emitter.label(&valid_label);
            emitter.instruction("bl __rt_json_decode_mixed");                  // structural decoder: scalars box natively; arrays decode as Mixed(array); objects honor _json_decode_assoc
            emitter.label(&done_label);
        }
        Arch::X86_64 => {
            // rax = string ptr, rdx = string len after emit_expr.
            emitter.instruction("push rax");                                   // park the json string pointer across the validator call
            emitter.instruction("push rdx");                                   // park the json string length across the validator call (kept aligned to 16 bytes)
            emitter.instruction("call __rt_json_validate");                    // RFC 8259 validator; returns 1 on success, 0 on failure (and sets _json_last_error)
            emitter.instruction("pop rdx");                                    // restore the json string length for the structural decoder
            emitter.instruction("pop rsi");                                    // pop the saved pointer into a scratch register before swapping into rax
            emitter.instruction("test rax, rax");                              // valid → non-zero; invalid → zero
            emitter.instruction(&format!("jne {}", valid_label));               // valid → fall through to structural decode (rsi has the saved ptr)
            // Invalid: return Mixed(null) without invoking the decoder.
            emitter.instruction("mov rax, 8");                                 // tag = 8 (null)
            emitter.instruction("mov rdi, 0");                                 // value_lo = 0
            emitter.instruction("mov rsi, 0");                                 // value_hi = 0
            emitter.instruction("call __rt_mixed_from_value");                 // box Mixed(null) so callers see a uniform result shape
            emitter.instruction(&format!("jmp {}", done_label));                // skip the structural decoder when validation already rejected the input
            emitter.label(&valid_label);
            emitter.instruction("mov rax, rsi");                               // restore the json string pointer into the rax/rdx string-arg pair
            emitter.instruction("call __rt_json_decode_mixed");                // structural decoder honoring _json_decode_assoc
            emitter.label(&done_label);
        }
    }

    Some(PhpType::Mixed)
}

/// Materialize the `$associative` argument into the runtime flag symbol.
///
/// PHP semantics: missing or `null` → false (stdClass), `false` → false,
/// `true` → true. Anything else gets the usual PHP coercion to bool. For
/// the common compile-time literal cases we set the flag with a constant
/// store; dynamic expressions evaluate at runtime and use the result
/// register's value as the flag.
fn write_assoc_flag(
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) {
    if args.len() < 2 || matches!(args[1].kind, ExprKind::Null) {
        abi::emit_store_zero_to_symbol(emitter, "_json_decode_assoc", 0);       // default associative=false → stdClass for all decoded objects
        return;
    }
    if let ExprKind::BoolLiteral(value) = args[1].kind {
        if value {
            // Materialize the literal `true` (1) via a small helper so
            // arch-specific stores stay confined to abi::*.
            let scratch = abi::int_result_reg(emitter);
            abi::emit_load_int_immediate(emitter, scratch, 1);
            abi::emit_store_reg_to_symbol(emitter, scratch, "_json_decode_assoc", 0); // record true so all decoded objects become assoc arrays
        } else {
            abi::emit_store_zero_to_symbol(emitter, "_json_decode_assoc", 0);   // explicit false → stdClass for all decoded objects
        }
        return;
    }
    // Dynamic argument: evaluate and store the bool/int result. Non-zero
    // values flag assoc, zero flags stdClass — matching PHP's truthiness
    // for the second parameter.
    let _ = emit_expr(&args[1], emitter, ctx, data);
    let scratch = abi::int_result_reg(emitter);
    abi::emit_store_reg_to_symbol(emitter, scratch, "_json_decode_assoc", 0);   // store the runtime-truthy result of $associative for the upcoming decode
}
