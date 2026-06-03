//! Purpose:
//! Computes and emits stack-frame setup/teardown for the EIR backend.
//! Reuses the target-aware ABI frame helpers from the legacy backend.
//!
//! Called from:
//! - `crate::codegen_ir::block_emit`.
//!
//! Key details:
//! - Frame size is value-placement bytes plus the target frame footer, rounded to 16 bytes.
//! - Main currently exits through the process syscall, matching the legacy entry path.

use crate::codegen::abi;
use crate::codegen::platform::Arch;

use super::context::FunctionContext;
use super::value_placement::ValuePlacement;

const FRAME_FOOTER_BYTES: usize = 16;

/// Returns the aligned frame size for a function's fixed value slots.
pub(super) fn frame_size_for_placement(placement: &ValuePlacement) -> usize {
    align_to_16(placement.total_slot_bytes + FRAME_FOOTER_BYTES)
}

/// Emits the process-entry prologue for the EIR main function.
pub(super) fn emit_main_prologue(ctx: &mut FunctionContext<'_>) {
    if ctx.emitter.target.arch == Arch::AArch64 {
        ctx.emitter.raw(".align 2");
    }
    ctx.emitter.blank();
    ctx.emitter.entry_label();
    abi::emit_frame_prologue(ctx.emitter, ctx.frame_size);
    ctx.emitter.comment("save argc/argv to globals");
    abi::emit_store_process_args_to_globals(ctx.emitter);
}

/// Emits frame teardown and exits the process with status 0.
pub(super) fn emit_main_epilogue(ctx: &mut FunctionContext<'_>) {
    if ctx.epilogue_emitted {
        return;
    }
    ctx.emitter.blank();
    ctx.emitter.comment("epilogue + exit(0)");
    abi::emit_frame_restore(ctx.emitter, ctx.frame_size);
    abi::emit_exit(ctx.emitter, 0);
    ctx.epilogue_emitted = true;
}

/// Rounds a byte count up to a 16-byte stack alignment boundary.
fn align_to_16(bytes: usize) -> usize {
    (bytes + 15) & !15
}
