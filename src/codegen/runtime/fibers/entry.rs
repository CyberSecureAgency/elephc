//! Fiber entry trampoline.
//!
//! Runs once per fiber, the first time it is switched into. Reads the captured
//! callable from the Fiber object, invokes it, captures the return value, marks
//! the fiber terminated, and switches back to the caller.

use crate::codegen::abi;
use crate::codegen::context::{TRY_HANDLER_JMP_BUF_OFFSET, TRY_HANDLER_SLOT_SIZE};
use crate::codegen::emit::Emitter;
use crate::codegen::platform::Arch;

use super::{
    FIBER_CALLABLE_OFFSET, FIBER_CALLER_OFFSET, FIBER_FLOAT_ARGS_OFFSET, FIBER_PENDING_THROW_OFFSET,
    FIBER_START_ARGS_OFFSET, FIBER_STATE_OFFSET, FIBER_STATE_RUNNING, FIBER_STATE_TERMINATED,
    FIBER_TRANSFER_VALUE_OFFSET,
};

/// __rt_fiber_entry: trampoline executed at the start of every fiber.
/// On entry the fiber's saved stack has just been restored by __rt_fiber_switch.
/// The active fiber is `_fiber_current`.
pub fn emit_fiber_entry(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_x86_64_stub(emitter);
        return;
    }

    emitter.blank();
    emitter.comment("--- runtime: fiber_entry ---");
    emitter.label_global("__rt_fiber_entry");

    // -- establish a tiny frame on this fiber's fresh stack --
    emitter.instruction("sub sp, sp, #16");                                     // reserve a minimal scratch frame on the fiber stack
    emitter.instruction("str x29, [sp, #0]");                                   // store a zero-equivalent FP slot for diagnostic walkers
    emitter.instruction("mov x29, sp");                                         // anchor the frame pointer at the new bottom of the fiber stack

    // -- install a sentinel exception handler so any exception that escapes the
    //    closure's own try/catch chain unwinds back here instead of terminating
    //    the process via the standard "uncaught exception" path. --
    // Use x10 (caller-saved scratch) for register sources passed to
    // emit_store_reg_to_symbol — that helper uses x9 internally for the symbol
    // address, so source register x9 would self-clobber.
    emitter.instruction(&format!("sub sp, sp, #{}", TRY_HANDLER_SLOT_SIZE));    // reserve TRY_HANDLER_SLOT_SIZE bytes on the fiber stack for the boundary handler
    abi::emit_load_symbol_to_reg(emitter, "x10", "_exc_handler_top", 0);        // x10 = previous head of the handler chain (the fiber's saved value, typically NULL on a fresh fiber)
    emitter.instruction("str x10, [sp, #0]");                                   // handler.next = previous chain head
    emitter.instruction("str xzr, [sp, #8]");                                   // handler.activation_record = NULL → cleanup_frames unwinds the entire fiber call stack
    abi::emit_load_symbol_to_reg(emitter, "x10", "_rt_diag_suppression", 0);    // x10 = current diagnostic-suppression depth
    emitter.instruction("str x10, [sp, #16]");                                  // handler.saved_diag_depth = current depth (matches user-emitted try frames)
    emitter.instruction("mov x10, sp");                                         // x10 = address of the handler base
    abi::emit_store_reg_to_symbol(emitter, "x10", "_exc_handler_top", 0);       // push the boundary handler onto the global handler chain
    emitter.instruction(&format!("add x0, sp, #{}", TRY_HANDLER_JMP_BUF_OFFSET)); // x0 = jmp_buf address inside the handler (offset 24)
    emitter.bl_c("setjmp");                                                     // setjmp returns 0 the first time; non-zero on a longjmp from __rt_throw_current
    emitter.instruction("cbnz x0, __rt_fiber_entry_escape");                    // a non-zero return means an exception unwound past every user handler

    // -- mark the fiber Running and load its captured callable --
    abi::emit_load_symbol_to_reg(emitter, "x19", "_fiber_current", 0);          // x19 = pointer to the fiber object that just started
    emitter.instruction(&format!("mov x20, #{}", FIBER_STATE_RUNNING));         // FIBER_STATE_RUNNING constant
    emitter.instruction(&format!("str x20, [x19, #{}]", FIBER_STATE_OFFSET));   // state = Running

    // -- load the captured callable and any start() arguments, then call --
    // The seven start_args slots always hold something (Mixed-null cells when the
    // user did not pass an explicit argument), so we can unconditionally load
    // x0..x6 before the call — that exhausts the AArch64 integer arg registers
    // available after $this. The parallel float_args[0..7] file feeds d0..d6
    // so float captures can ride alongside int/string captures. Closures with
    // fewer parameters simply ignore the extra registers per the caller-saved
    // convention.
    emitter.instruction(&format!("ldr x9, [x19, #{}]", FIBER_CALLABLE_OFFSET)); // x9 = closure function pointer
    emitter.instruction(&format!("ldr x0, [x19, #{}]", FIBER_START_ARGS_OFFSET)); // x0 = start_args[0]
    emitter.instruction(&format!("ldr x1, [x19, #{}]", FIBER_START_ARGS_OFFSET + 8)); // x1 = start_args[1]
    emitter.instruction(&format!("ldr x2, [x19, #{}]", FIBER_START_ARGS_OFFSET + 16)); // x2 = start_args[2]
    emitter.instruction(&format!("ldr x3, [x19, #{}]", FIBER_START_ARGS_OFFSET + 24)); // x3 = start_args[3]
    emitter.instruction(&format!("ldr x4, [x19, #{}]", FIBER_START_ARGS_OFFSET + 32)); // x4 = start_args[4]
    emitter.instruction(&format!("ldr x5, [x19, #{}]", FIBER_START_ARGS_OFFSET + 40)); // x5 = start_args[5]
    emitter.instruction(&format!("ldr x6, [x19, #{}]", FIBER_START_ARGS_OFFSET + 48)); // x6 = start_args[6]
    emitter.instruction(&format!("ldr d0, [x19, #{}]", FIBER_FLOAT_ARGS_OFFSET)); // d0 = float_args[0]
    emitter.instruction(&format!("ldr d1, [x19, #{}]", FIBER_FLOAT_ARGS_OFFSET + 8)); // d1 = float_args[1]
    emitter.instruction(&format!("ldr d2, [x19, #{}]", FIBER_FLOAT_ARGS_OFFSET + 16)); // d2 = float_args[2]
    emitter.instruction(&format!("ldr d3, [x19, #{}]", FIBER_FLOAT_ARGS_OFFSET + 24)); // d3 = float_args[3]
    emitter.instruction(&format!("ldr d4, [x19, #{}]", FIBER_FLOAT_ARGS_OFFSET + 32)); // d4 = float_args[4]
    emitter.instruction(&format!("ldr d5, [x19, #{}]", FIBER_FLOAT_ARGS_OFFSET + 40)); // d5 = float_args[5]
    emitter.instruction(&format!("ldr d6, [x19, #{}]", FIBER_FLOAT_ARGS_OFFSET + 48)); // d6 = float_args[6]
    emitter.instruction("blr x9");                                              // call the closure with up to 7 int and 7 float captured args

    // -- store the return value into transfer_value (lo half) and mark Terminated --
    abi::emit_load_symbol_to_reg(emitter, "x19", "_fiber_current", 0);          // reload x19 — registers were clobbered across the closure call
    emitter.instruction(&format!("str x0, [x19, #{}]", FIBER_TRANSFER_VALUE_OFFSET)); // transfer_value.lo = closure return value
    emitter.instruction(&format!("str xzr, [x19, #{}]", FIBER_TRANSFER_VALUE_OFFSET + 8)); // transfer_value.hi = 0 (raw integer/string default tag)
    emitter.instruction(&format!("mov x20, #{}", FIBER_STATE_TERMINATED));      // FIBER_STATE_TERMINATED constant
    emitter.instruction(&format!("str x20, [x19, #{}]", FIBER_STATE_OFFSET));   // state = Terminated

    // -- pop the boundary handler before yielding control back to the caller --
    // Use x10 — emit_store_reg_to_symbol uses x9 internally for the symbol address.
    emitter.instruction("ldr x10, [sp, #0]");                                   // x10 = handler.next (previous chain head)
    abi::emit_store_reg_to_symbol(emitter, "x10", "_exc_handler_top", 0);       // restore the previous handler chain head

    // -- switch back to whoever resumed us (caller can never be NULL inside a fiber) --
    emitter.instruction(&format!("ldr x0, [x19, #{}]", FIBER_CALLER_OFFSET));   // x0 = caller fiber* (or NULL = main)
    emitter.instruction("bl __rt_fiber_switch");                                // hand control back; this call never returns inside this fiber

    // -- defensive trap: a terminated fiber must never resume past the switch --
    emitter.label("__rt_fiber_entry_unreachable");
    emitter.instruction("brk #0xfffe");                                         // trap if the unreachable epilogue is ever entered

    // -- escape path: longjmp landed here because no user handler matched --
    emitter.label("__rt_fiber_entry_escape");
    abi::emit_load_symbol_to_reg(emitter, "x10", "_exc_value", 0);              // x10 = the Throwable that was unwound past every user catch
    abi::emit_load_symbol_to_reg(emitter, "x19", "_fiber_current", 0);          // x19 = current fiber* (preserved through longjmp via the global)
    emitter.instruction(&format!("str x10, [x19, #{}]", FIBER_PENDING_THROW_OFFSET)); // park the escaped Throwable so the caller's helper can re-raise it
    emitter.instruction(&format!("str xzr, [x19, #{}]", FIBER_TRANSFER_VALUE_OFFSET)); // wipe transfer_value.lo so callers do not see stale data
    emitter.instruction(&format!("str xzr, [x19, #{}]", FIBER_TRANSFER_VALUE_OFFSET + 8)); // wipe transfer_value.hi as well
    emitter.instruction(&format!("mov x20, #{}", FIBER_STATE_TERMINATED));      // FIBER_STATE_TERMINATED constant — the fiber is done after an escape
    emitter.instruction(&format!("str x20, [x19, #{}]", FIBER_STATE_OFFSET));   // state = Terminated

    // -- pop the boundary handler from the chain (longjmp restored SP to setjmp time) --
    // Use x10 — emit_store_reg_to_symbol uses x9 internally for the symbol address.
    emitter.instruction("ldr x10, [sp, #0]");                                   // x10 = handler.next
    abi::emit_store_reg_to_symbol(emitter, "x10", "_exc_handler_top", 0);       // restore the previous handler chain head
    emitter.instruction("ldr x10, [sp, #16]");                                  // x10 = saved diagnostic suppression depth
    abi::emit_store_reg_to_symbol(emitter, "x10", "_rt_diag_suppression", 0);   // restore the diagnostic suppression depth captured at setjmp time

    // -- switch back to the caller; their helper sees Terminated + non-null pending_throw and re-raises --
    emitter.instruction(&format!("ldr x0, [x19, #{}]", FIBER_CALLER_OFFSET));   // x0 = caller fiber* (or NULL = main)
    emitter.instruction("bl __rt_fiber_switch");                                // hand control back; the caller-side helper handles re-raising
    emitter.instruction("brk #0xfffe");                                         // defensive trap: a terminated fiber must never resume past the switch
}

/// Invoke the callable captured by the fiber.
///
/// In elephc, a value of type `PhpType::Callable` is the raw function pointer of
/// the lowered closure body — there is no extra heap header to dereference. The
/// MVP supports closures without captures; closures that capture by value would
/// require their captures to be passed as trailing arguments here, which is not
/// yet wired up.
///
/// Input:  x0 = callable function pointer
/// Output: x0 = closure return value (raw scalar / pointer)
pub fn emit_fiber_invoke_callable_stub(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_x86_64_stub_named(emitter, "__rt_fiber_invoke_callable");
        return;
    }

    emitter.blank();
    emitter.comment("--- runtime: fiber_invoke_callable ---");
    emitter.label_global("__rt_fiber_invoke_callable");

    emitter.instruction("cbz x0, __rt_fiber_invoke_callable_null");             // a NULL callable returns 0 immediately
    emitter.instruction("br x0");                                               // tail-call directly into the closure body (x0 is already the function address)
    emitter.label("__rt_fiber_invoke_callable_null");
    emitter.instruction("mov x0, #0");                                          // return zero when the fiber was constructed without a callable
    emitter.instruction("ret");                                                 // hand control back to the entry trampoline
}

fn emit_x86_64_stub(emitter: &mut Emitter) {
    emit_x86_64_stub_named(emitter, "__rt_fiber_entry");
}

fn emit_x86_64_stub_named(emitter: &mut Emitter, name: &str) {
    emitter.blank();
    emitter.comment(&format!("--- runtime: {} (x86_64 stub) ---", name));
    emitter.label_global(name);
    emitter.instruction("ret");                                                 // x86_64 fiber runtime not yet implemented
}
