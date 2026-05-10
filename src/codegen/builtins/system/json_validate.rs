use crate::codegen::abi;
use crate::codegen::context::Context;
use crate::codegen::data_section::DataSection;
use crate::codegen::emit::Emitter;
use crate::codegen::expr::emit_expr;
use crate::codegen::platform::Arch;
use crate::parser::ast::Expr;
use crate::types::PhpType;

pub fn emit(
    _name: &str,
    args: &[Expr],
    emitter: &mut Emitter,
    ctx: &mut Context,
    data: &mut DataSection,
) -> Option<PhpType> {
    emitter.comment("json_validate()");

    // PHP resets json_last_error() at the start of every call so a previous
    // failure does not leak into the next one's success result.
    abi::emit_store_zero_to_symbol(emitter, "_json_last_error", 0);

    // Evaluate args[2] (flags) before args[0] (json) so the runtime can
    // consult `_json_active_flags`. The flag argument is virtually always a
    // constant — see json_encode for the same justification.
    if let Some(flag_expr) = args.get(2) {
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
    // Reset the active recursion depth so a previous json_encode pass does
    // not leak depth state into a fresh validate call.
    abi::emit_store_zero_to_symbol(emitter, "_json_active_depth", 0);
    // args[1] is the recursion depth limit. Default to 512 (matches PHP).
    // PHP json_validate rejects nesting when active_depth >= depth (strict).
    // The shared __rt_json_depth_enter compares `active <= limit` so we
    // subtract 1 from the user-supplied depth to align (depth=1 → limit=0
    // → top-level container fails).
    if let Some(depth_expr) = args.get(1) {
        emit_expr(depth_expr, emitter, ctx, data);
        let reg = abi::int_result_reg(emitter);
        match emitter.target.arch {
            Arch::AArch64 => emitter.instruction(&format!("sub {reg}, {reg}, #1")), // strict-semantic offset for json_validate
            Arch::X86_64 => emitter.instruction(&format!("sub {reg}, 1")),         // strict-semantic offset for json_validate
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

    // Evaluate the JSON string into the standard string-result registers
    // (x1=ptr, x2=len on ARM64).
    emit_expr(&args[0], emitter, ctx, data);
    abi::emit_call_label(emitter, "__rt_json_validate");
    Some(PhpType::Bool)
}
