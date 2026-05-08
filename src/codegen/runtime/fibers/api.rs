//! Fiber public-API runtime helpers.
//!
//! These are the functions the codegen will call when lowering Fiber method
//! invocations. Phase 1 wires up the *constructor*, which fully prepares a
//! fiber object and a fake initial frame so a future switch can resume into
//! `__rt_fiber_entry`. The remaining helpers (`start`, `resume`, `suspend`,
//! `throw`, `getCurrent`, `getReturn`, state predicates) are emitted as
//! placeholder stubs that link cleanly while later phases fill in semantics.

use crate::codegen::abi;
use crate::codegen::emit::Emitter;
use crate::codegen::platform::Arch;

use super::switch::{fiber_initial_entry_offset, fiber_initial_stack_frame_bytes};
use super::{
    FIBER_CALLABLE_OFFSET, FIBER_CALLABLE_WRAPPER_OFFSET, FIBER_CALLER_OFFSET, FIBER_DEFAULT_STACK_SIZE,
    FIBER_FLOAT_ARGS_MAX, FIBER_FLOAT_ARGS_OFFSET, FIBER_OBJECT_SIZE,
    FIBER_OWN_CALL_FRAME_OFFSET, FIBER_OWN_EXC_HEAD_OFFSET, FIBER_PENDING_THROW_OFFSET,
    FIBER_SAVED_SP_OFFSET, FIBER_STACK_BASE_OFFSET, FIBER_STACK_SIZE_OFFSET,
    FIBER_STACK_TOP_OFFSET, FIBER_START_ARGS_MAX, FIBER_START_ARGS_OFFSET,
    FIBER_STATE_NOT_STARTED, FIBER_STATE_OFFSET, FIBER_STATE_RUNNING, FIBER_STATE_SUSPENDED,
    FIBER_STATE_TERMINATED, FIBER_TRANSFER_VALUE_OFFSET, FIBER_USER_ARG_MAX_OFFSET,
};

const X86_64_HEAP_MAGIC_HI32: u64 = 0x454C5048;

/// __rt_fiber_throw_state_error: allocate a `FiberError`, set its message, and
/// raise it through the standard exception runtime. Never returns.
/// Input:  x0 = message bytes pointer, x1 = message length
pub fn emit_fiber_throw_state_error(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_throw_state_error_x86_64(emitter);
        return;
    }

    emitter.blank();
    emitter.comment("--- runtime: fiber_throw_state_error ---");
    emitter.label_global("__rt_fiber_throw_state_error");

    emitter.instruction("sub sp, sp, #32");                                     // reserve frame plus saved-callee slots
    emitter.instruction("stp x29, x30, [sp, #16]");                             // save frame pointer and return address
    emitter.instruction("stp x19, x20, [sp]");                                  // preserve caller's x19/x20 — we use them to cache the message across heap_alloc
    emitter.instruction("add x29, sp, #16");                                    // anchor the new frame pointer
    emitter.instruction("mov x19, x0");                                         // x19 = message bytes pointer (callee-saved across __rt_heap_alloc)
    emitter.instruction("mov x20, x1");                                         // x20 = message length (callee-saved across __rt_heap_alloc)

    emitter.instruction("mov x0, #24");                                         // FiberError object size (8 class_id + 16 message property)
    emitter.instruction("bl __rt_heap_alloc");                                  // x0 = freshly allocated payload pointer (heap header at -8)

    emitter.instruction("mov x9, #4");                                          // heap kind 4 = object instance
    emitter.instruction("str x9, [x0, #-8]");                                   // stamp the kind in the uniform heap header
    abi::emit_load_symbol_to_reg(emitter, "x9", "_fiber_error_class_id", 0);    // x9 = runtime class id of FiberError
    emitter.instruction("str x9, [x0]");                                        // store FiberError class id at the object header

    emitter.instruction("str x19, [x0, #8]");                                   // message property low half = bytes pointer (matches Exception's message slot layout)
    emitter.instruction("str x20, [x0, #16]");                                  // message property high half = byte length

    abi::emit_store_reg_to_symbol(emitter, "x0", "_exc_value", 0);              // _exc_value = the freshly built FiberError, matching the standard `throw` runtime contract
    emitter.instruction("bl __rt_throw_current");                               // unwind into the active try/catch chain (no return)

    emitter.instruction("brk #0xfffe");                                         // defensive trap: __rt_throw_current must not return here
}

/// __rt_fiber_construct: allocate and initialise a Fiber object.
/// Input:  x0 = callable (closure object pointer; may be NULL for diagnostics)
///         x1 = class_id assigned by the type checker for the Fiber class
///         x2 = wrapper entry that adapts Fiber Mixed traffic to the callable ABI
/// Output: x0 = pointer to the new Fiber object (16-byte heap header sits at -8)
pub fn emit_fiber_construct(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_construct_x86_64(emitter);
        return;
    }

    let initial_frame_bytes = fiber_initial_stack_frame_bytes(emitter.target.arch);
    let initial_entry_offset = fiber_initial_entry_offset(emitter.target.arch);

    emitter.blank();
    emitter.comment("--- runtime: fiber_construct ---");
    emitter.label_global("__rt_fiber_construct");

    // -- frame: keep callable/class/wrapper across the heap calls --
    emitter.instruction("sub sp, sp, #64");                                     // reserve scratch frame plus saved callee regs
    emitter.instruction("stp x29, x30, [sp, #48]");                             // save frame pointer and return address
    emitter.instruction("stp x19, x20, [sp]");                                  // preserve callee-saved registers used as argument cache
    emitter.instruction("stp x21, x22, [sp, #16]");                             // preserve callee-saved registers used for object and wrapper pointers
    emitter.instruction("add x29, sp, #48");                                    // anchor the new frame pointer
    emitter.instruction("mov x19, x0");                                         // x19 = callable (preserved across heap_alloc)
    emitter.instruction("mov x20, x1");                                         // x20 = class_id (preserved across heap_alloc)
    emitter.instruction("mov x22, x2");                                         // x22 = optional Fiber entry wrapper pointer

    // -- allocate the Fiber object payload --
    emitter.instruction(&format!("mov x0, #{}", FIBER_OBJECT_SIZE));            // size in bytes for the Fiber object payload
    emitter.instruction("bl __rt_heap_alloc");                                  // x0 = pointer to the object payload (header at x0-8)
    emitter.instruction("mov x21, x0");                                         // x21 = Fiber object pointer (kept until return)
    emitter.instruction("mov x9, #4");                                          // heap kind 4 = object instance
    emitter.instruction("str x9, [x21, #-8]");                                  // stamp the kind in the uniform heap header
    emitter.instruction("str x20, [x21]");                                      // store the runtime class_id at the object header

    // -- zero-initialise every Fiber field before populating the meaningful ones --
    emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_STATE_OFFSET));   // state placeholder (overwritten below with NotStarted)
    emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_STACK_BASE_OFFSET)); // stack_base placeholder (overwritten after stack alloc)
    emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_STACK_TOP_OFFSET)); // stack_top placeholder (overwritten after stack alloc)
    emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_STACK_SIZE_OFFSET)); // stack_size placeholder (overwritten after stack alloc)
    emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_SAVED_SP_OFFSET)); // saved_sp placeholder (overwritten after fake-frame setup)
    emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_CALLABLE_OFFSET)); // callable.lo placeholder (overwritten with x19 below)
    emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_CALLABLE_WRAPPER_OFFSET)); // callable wrapper placeholder (overwritten with x22 below)
    emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_CALLER_OFFSET));  // caller starts NULL (no resumer until start/resume)
    emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_TRANSFER_VALUE_OFFSET)); // transfer_value.lo cleared
    emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_TRANSFER_VALUE_OFFSET + 8)); // transfer_value.hi cleared
    emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_PENDING_THROW_OFFSET)); // pending_throw cleared
    emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_OWN_EXC_HEAD_OFFSET)); // own_exc_head cleared (no installed handlers yet)
    emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_OWN_CALL_FRAME_OFFSET)); // own_call_frame cleared (no activation records on the fresh fiber stack yet)
    for i in 0..FIBER_START_ARGS_MAX {
        emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_START_ARGS_OFFSET + i * 8)); // start_args[i] cleared
    }
    for i in 0..FIBER_FLOAT_ARGS_MAX {
        emitter.instruction(&format!("str xzr, [x21, #{}]", FIBER_FLOAT_ARGS_OFFSET + i * 8)); // float_args[i] cleared (raw bits, loaded back into d-regs by the trampoline)
    }
    // user_arg_max defaults to FIBER_START_ARGS_MAX so start() fills every
    // start_args slot when no captures are pre-loaded into the trailing ones.
    // Codegen of `new Fiber(function() use(...) {})` lowers it to the
    // closure's user-param count to keep the captures intact.
    emitter.instruction(&format!("mov x9, #{}", FIBER_START_ARGS_MAX));         // default user_arg_max = full slot count
    emitter.instruction(&format!("str x9, [x21, #{}]", FIBER_USER_ARG_MAX_OFFSET)); // user_arg_max stored on the freshly built fiber

    // -- record the captured callable --
    emitter.instruction(&format!("str x19, [x21, #{}]", FIBER_CALLABLE_OFFSET)); // callable.lo = closure pointer
    emitter.instruction(&format!("str x22, [x21, #{}]", FIBER_CALLABLE_WRAPPER_OFFSET)); // callable wrapper = Fiber entry ABI adapter

    // -- allocate the per-fiber stack via mmap; alloc returns base/top/total --
    emitter.instruction(&format!("mov x0, #{}", FIBER_DEFAULT_STACK_SIZE));     // request the default usable fiber stack size in bytes
    emitter.instruction("bl __rt_fiber_alloc_stack");                           // x0 = stack_base (mapping start), x1 = stack_top, x2 = total mapped length
    emitter.instruction("cbz x0, __rt_fiber_construct_stack_failed");           // abort construction if mmap failed instead of writing a fake frame at NULL
    emitter.instruction(&format!("str x0, [x21, #{}]", FIBER_STACK_BASE_OFFSET)); // stack_base = mmap mapping start (includes the guard page)
    emitter.instruction(&format!("str x1, [x21, #{}]", FIBER_STACK_TOP_OFFSET)); // stack_top = initial SP target (16-byte aligned high address)
    emitter.instruction(&format!("str x2, [x21, #{}]", FIBER_STACK_SIZE_OFFSET)); // stack_size = total mapped length, needed verbatim by munmap on free

    // -- carve out a fake initial frame at the very top of the stack --
    emitter.instruction(&format!("sub x10, x1, #{}", initial_frame_bytes));     // x10 = initial saved_sp (room for the switch save area)
    emitter.instruction(&format!("str x10, [x21, #{}]", FIBER_SAVED_SP_OFFSET)); // saved_sp points at the bottom of the fake frame

    // -- zero the fake frame so callee-saved registers come back as zero on first switch --
    emitter.instruction("mov x11, x10");                                        // x11 = cursor through the fake frame
    emitter.instruction(&format!("mov x12, #{}", initial_frame_bytes / 16));    // number of 16-byte chunks to zero
    emitter.label("__rt_fiber_construct_zero_loop");
    emitter.instruction("stp xzr, xzr, [x11], #16");                            // zero a 16-byte slice and advance the cursor
    emitter.instruction("subs x12, x12, #1");                                   // decrement the chunk counter
    emitter.instruction("b.ne __rt_fiber_construct_zero_loop");                 // continue until the entire frame is zero

    // -- install __rt_fiber_entry as the saved x30 so the first switch returns into it --
    abi::emit_symbol_address(emitter, "x9", "__rt_fiber_entry");                // x9 = absolute address of the entry trampoline
    emitter.instruction(&format!("str x9, [x10, #{}]", initial_entry_offset));  // saved x30 slot = entry trampoline address

    // -- finish: state = NotStarted and return the new Fiber pointer --
    emitter.instruction(&format!("mov x9, #{}", FIBER_STATE_NOT_STARTED));      // FIBER_STATE_NOT_STARTED constant
    emitter.instruction(&format!("str x9, [x21, #{}]", FIBER_STATE_OFFSET));    // state = NotStarted
    emitter.instruction("mov x0, x21");                                         // return the freshly built Fiber pointer

    // -- tear down the scratch frame and return --
    emitter.instruction("ldp x21, x22, [sp, #16]");                             // restore caller's x21/x22
    emitter.instruction("ldp x19, x20, [sp]");                                  // restore caller's x19/x20
    emitter.instruction("ldp x29, x30, [sp, #48]");                             // restore caller's frame pointer and return address
    emitter.instruction("add sp, sp, #64");                                     // release the scratch frame
    emitter.instruction("ret");                                                 // hand the new Fiber object back to the constructor caller

    emitter.label("__rt_fiber_construct_stack_failed");
    abi::emit_symbol_address(emitter, "x0", "_fiber_msg_stack_alloc_failed");   // x0 = pointer to the static stack-allocation failure message
    emitter.instruction("mov x1, #27");                                         // x1 = error message length in bytes
    emitter.instruction("bl __rt_fiber_throw_state_error");                     // raise FiberError instead of dereferencing a NULL stack top
    emitter.instruction("brk #0xfffe");                                         // defensive trap: the throw helper must not return
}

/// __rt_fiber_start: switch into a fiber for the first time.
/// Input:  x0 = fiber*
/// Output: x0 = the value the fiber yielded (via Fiber::suspend) or returned.
pub fn emit_fiber_start(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_start_x86_64(emitter);
        return;
    }

    emitter.blank();
    emitter.comment("--- runtime: fiber_start ---");
    emitter.label_global("__rt_fiber_start");

    // -- prologue: keep the fiber pointer in x19 across the cooperative switch --
    emitter.instruction("sub sp, sp, #32");                                     // reserve a frame plus a saved-x19 slot
    emitter.instruction("stp x29, x30, [sp, #16]");                             // save frame pointer and return address
    emitter.instruction("str x19, [sp]");                                       // preserve caller's x19 — we are about to repurpose it
    emitter.instruction("add x29, sp, #16");                                    // anchor the new frame pointer
    emitter.instruction("mov x19, x0");                                         // x19 = fiber object pointer (callee-saved across __rt_fiber_switch)

    // -- guard: start() requires state == NotStarted; otherwise raise FiberError --
    emitter.instruction(&format!("ldr x9, [x19, #{}]", FIBER_STATE_OFFSET));    // x9 = receiver fiber state
    emitter.instruction(&format!("cmp x9, #{}", FIBER_STATE_NOT_STARTED));      // is the fiber still in the NotStarted state?
    emitter.instruction("b.eq __rt_fiber_start_state_ok");                      // proceed when the fiber has not been started yet
    abi::emit_symbol_address(emitter, "x0", "_fiber_msg_already_started");      // x0 = pointer to the static error message
    emitter.instruction("mov x1, #50");                                         // x1 = error message length in bytes
    emitter.instruction("bl __rt_fiber_throw_state_error");                     // raise FiberError; this call does not return
    emitter.label("__rt_fiber_start_state_ok");

    // -- record the resumer (current execution context) as the fiber's caller --
    abi::emit_load_symbol_to_reg(emitter, "x9", "_fiber_current", 0);           // x9 = whoever is running right now (NULL means main thread)
    emitter.instruction(&format!("str x9, [x19, #{}]", FIBER_CALLER_OFFSET));   // fiber->caller = current execution context

    // -- switch into the fiber; control returns when it suspends or terminates --
    emitter.instruction("mov x0, x19");                                         // pass fiber* as the switch target
    emitter.instruction("bl __rt_fiber_switch");                                // cooperative context switch into the fiber

    // -- check for an escaped exception parked by the trampoline and re-raise it on the caller's stack --
    emitter.instruction(&format!("ldr x9, [x19, #{}]", FIBER_STATE_OFFSET));    // x9 = current fiber state
    emitter.instruction(&format!("cmp x9, #{}", FIBER_STATE_TERMINATED));       // is the fiber terminated?
    emitter.instruction("b.ne __rt_fiber_start_no_escape");                     // skip the re-raise path when the fiber is still alive
    emitter.instruction(&format!("ldr x10, [x19, #{}]", FIBER_PENDING_THROW_OFFSET)); // x10 = parked Throwable (NULL when termination was clean)
    emitter.instruction("cbz x10, __rt_fiber_start_no_escape");                 // skip the re-raise path when no exception escaped
    emitter.instruction(&format!("str xzr, [x19, #{}]", FIBER_PENDING_THROW_OFFSET)); // clear pending_throw so subsequent inspections see the clean terminated state
    abi::emit_store_reg_to_symbol(emitter, "x10", "_exc_value", 0);             // _exc_value = the escaped Throwable, ready for __rt_throw_current
    emitter.instruction("bl __rt_throw_current");                               // re-raise on the caller's stack chain (no return)
    emitter.instruction("brk #0xfffe");                                         // defensive trap if __rt_throw_current ever returns
    emitter.label("__rt_fiber_start_no_escape");

    // -- harvest a suspend value, or PHP null when the fiber terminated cleanly --
    emitter.instruction(&format!("ldr x9, [x19, #{}]", FIBER_STATE_OFFSET));    // x9 = current fiber state after control returned
    emitter.instruction(&format!("cmp x9, #{}", FIBER_STATE_TERMINATED));       // did the fiber finish instead of suspending?
    emitter.instruction("b.ne __rt_fiber_start_return_yield");                  // suspended fibers return their yielded transfer value
    emit_box_null_mixed(emitter);
    emitter.instruction("b __rt_fiber_start_return_ready");                     // skip the yielded-value load after boxing PHP null
    emitter.label("__rt_fiber_start_return_yield");
    emitter.instruction(&format!("ldr x0, [x19, #{}]", FIBER_TRANSFER_VALUE_OFFSET)); // x0 = fiber->transfer_value.lo (suspend yield value)
    emitter.label("__rt_fiber_start_return_ready");

    // -- epilogue --
    emitter.instruction("ldr x19, [sp]");                                       // restore caller's x19
    emitter.instruction("ldp x29, x30, [sp, #16]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #32");                                     // release the helper frame
    emitter.instruction("ret");                                                 // return the harvested value to the caller
}

/// __rt_fiber_resume: deliver a value into a suspended fiber and let it run.
/// Input:  x0 = fiber*, x1 = value to deliver to the suspended `Fiber::suspend()` call
/// Output: x0 = the value the fiber yielded next (via suspend/return)
pub fn emit_fiber_resume(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_resume_x86_64(emitter);
        return;
    }

    emitter.blank();
    emitter.comment("--- runtime: fiber_resume ---");
    emitter.label_global("__rt_fiber_resume");

    emitter.instruction("sub sp, sp, #32");                                     // reserve frame plus saved-x19 slot
    emitter.instruction("stp x29, x30, [sp, #16]");                             // save frame pointer and return address
    emitter.instruction("str x19, [sp]");                                       // preserve caller's x19 across the switch
    emitter.instruction("add x29, sp, #16");                                    // anchor the new frame pointer
    emitter.instruction("mov x19, x0");                                         // x19 = fiber* (callee-saved across the switch)

    // -- guard: resume() requires state == Suspended; otherwise raise FiberError.
    //    Hold the resume value in x20 across the helper because x1 is the second
    //    argument register, which the throw helper would clobber.
    emitter.instruction("stp x20, x21, [sp, #-16]!");                           // preserve caller's x20/x21 — both are callee-saved registers we are about to repurpose
    emitter.instruction("mov x20, x1");                                         // x20 = $value to deliver, parked across the state check
    emitter.instruction(&format!("ldr x9, [x19, #{}]", FIBER_STATE_OFFSET));    // x9 = receiver fiber state
    emitter.instruction(&format!("cmp x9, #{}", FIBER_STATE_SUSPENDED));        // is the fiber currently paused at a Fiber::suspend() call?
    emitter.instruction("b.eq __rt_fiber_resume_state_ok");                     // proceed only when the fiber is suspended
    abi::emit_symbol_address(emitter, "x0", "_fiber_msg_not_suspended");        // x0 = pointer to the static error message
    emitter.instruction("mov x1, #43");                                         // x1 = error message length in bytes
    emitter.instruction("bl __rt_fiber_throw_state_error");                     // raise FiberError; this call does not return
    emitter.label("__rt_fiber_resume_state_ok");
    emitter.instruction("mov x1, x20");                                         // restore the resume value into the second argument register
    emitter.instruction("ldp x20, x21, [sp], #16");                             // restore caller's x20/x21 now that the state check is done

    // -- deliver the new value into the fiber's transfer slot --
    emitter.instruction(&format!("str x1, [x19, #{}]", FIBER_TRANSFER_VALUE_OFFSET)); // fiber->transfer_value.lo = $value passed to resume

    // -- record the resumer as the fiber's caller, then switch in --
    abi::emit_load_symbol_to_reg(emitter, "x9", "_fiber_current", 0);           // x9 = current execution context to remember as the caller
    emitter.instruction(&format!("str x9, [x19, #{}]", FIBER_CALLER_OFFSET));   // fiber->caller = current context (so suspend knows who to yield back to)
    emitter.instruction("mov x0, x19");                                         // pass fiber* as the switch target
    emitter.instruction("bl __rt_fiber_switch");                                // cooperative context switch into the fiber

    // -- check for an escaped exception parked by the trampoline and re-raise it on the caller's stack --
    emitter.instruction(&format!("ldr x9, [x19, #{}]", FIBER_STATE_OFFSET));    // x9 = current fiber state
    emitter.instruction(&format!("cmp x9, #{}", FIBER_STATE_TERMINATED));       // is the fiber terminated?
    emitter.instruction("b.ne __rt_fiber_resume_no_escape");                    // skip the re-raise path when the fiber is still alive
    emitter.instruction(&format!("ldr x10, [x19, #{}]", FIBER_PENDING_THROW_OFFSET)); // x10 = parked Throwable (NULL when termination was clean)
    emitter.instruction("cbz x10, __rt_fiber_resume_no_escape");                // skip the re-raise path when no exception escaped
    emitter.instruction(&format!("str xzr, [x19, #{}]", FIBER_PENDING_THROW_OFFSET)); // clear pending_throw so subsequent inspections see the clean terminated state
    abi::emit_store_reg_to_symbol(emitter, "x10", "_exc_value", 0);             // _exc_value = the escaped Throwable, ready for __rt_throw_current
    emitter.instruction("bl __rt_throw_current");                               // re-raise on the caller's stack chain (no return)
    emitter.instruction("brk #0xfffe");                                         // defensive trap if __rt_throw_current ever returns
    emitter.label("__rt_fiber_resume_no_escape");

    // -- harvest a suspend value, or PHP null when the fiber terminated cleanly --
    emitter.instruction(&format!("ldr x9, [x19, #{}]", FIBER_STATE_OFFSET));    // x9 = current fiber state after control returned
    emitter.instruction(&format!("cmp x9, #{}", FIBER_STATE_TERMINATED));       // did the fiber finish instead of suspending again?
    emitter.instruction("b.ne __rt_fiber_resume_return_yield");                 // suspended fibers return their next yielded transfer value
    emit_box_null_mixed(emitter);
    emitter.instruction("b __rt_fiber_resume_return_ready");                    // skip the yielded-value load after boxing PHP null
    emitter.label("__rt_fiber_resume_return_yield");
    emitter.instruction(&format!("ldr x0, [x19, #{}]", FIBER_TRANSFER_VALUE_OFFSET)); // x0 = fiber->transfer_value.lo (next yield value)
    emitter.label("__rt_fiber_resume_return_ready");

    emitter.instruction("ldr x19, [sp]");                                       // restore caller's x19
    emitter.instruction("ldp x29, x30, [sp, #16]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #32");                                     // release the helper frame
    emitter.instruction("ret");                                                 // return the harvested value
}

/// __rt_fiber_suspend: yield control from the running fiber back to its caller.
/// Input:  x0 = value to deliver to the resumer's `start()` / `resume()` call
/// Output: x0 = the value the next resumer passes back via `resume($v)`
pub fn emit_fiber_suspend(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_suspend_x86_64(emitter);
        return;
    }

    emitter.blank();
    emitter.comment("--- runtime: fiber_suspend ---");
    emitter.label_global("__rt_fiber_suspend");

    emitter.instruction("sub sp, sp, #32");                                     // reserve frame plus saved-x19 slot
    emitter.instruction("stp x29, x30, [sp, #16]");                             // save frame pointer and return address
    emitter.instruction("str x19, [sp]");                                       // preserve caller's x19 across the switch
    emitter.instruction("add x29, sp, #16");                                    // anchor the new frame pointer

    // -- guard: suspend() must be called from inside a fiber; otherwise raise FiberError --
    // Hold the yielded value in x20 across the helper because x0 is its first argument.
    emitter.instruction("stp x20, x21, [sp, #-16]!");                           // preserve caller's x20/x21 — both are callee-saved registers we are about to repurpose
    emitter.instruction("mov x20, x0");                                         // x20 = yielded value, parked across the state check
    abi::emit_load_symbol_to_reg(emitter, "x19", "_fiber_current", 0);          // x19 = currently running fiber* (NULL means called from main)
    emitter.instruction("cbnz x19, __rt_fiber_suspend_state_ok");               // proceed when we are actually executing inside a fiber
    abi::emit_symbol_address(emitter, "x0", "_fiber_msg_suspend_outside");      // x0 = pointer to the static error message
    emitter.instruction("mov x1, #33");                                         // x1 = error message length in bytes
    emitter.instruction("bl __rt_fiber_throw_state_error");                     // raise FiberError; this call does not return
    emitter.label("__rt_fiber_suspend_state_ok");
    emitter.instruction("mov x0, x20");                                         // restore the yielded value into x0 for the suspend logic below
    emitter.instruction("ldp x20, x21, [sp], #16");                             // restore caller's x20/x21 now that the state check is done

    // -- store the value being yielded and mark the fiber Suspended --
    emitter.instruction(&format!("str x0, [x19, #{}]", FIBER_TRANSFER_VALUE_OFFSET)); // fiber->transfer_value.lo = the yielded value
    emitter.instruction(&format!("mov x9, #{}", FIBER_STATE_SUSPENDED));        // FIBER_STATE_SUSPENDED constant
    emitter.instruction(&format!("str x9, [x19, #{}]", FIBER_STATE_OFFSET));    // fiber->state = Suspended

    // -- switch back to the caller; control resumes here when someone calls resume() --
    emitter.instruction(&format!("ldr x0, [x19, #{}]", FIBER_CALLER_OFFSET));   // x0 = fiber->caller (whoever should regain control)
    emitter.instruction("bl __rt_fiber_switch");                                // hand control back to the caller's resume site

    // -- on resume, mark Running again --
    emitter.instruction(&format!("mov x9, #{}", FIBER_STATE_RUNNING));          // FIBER_STATE_RUNNING constant
    emitter.instruction(&format!("str x9, [x19, #{}]", FIBER_STATE_OFFSET));    // fiber->state = Running (we are executing again)

    // -- if a Throwable was scheduled by Fiber->throw($e), raise it inside this fiber --
    // Important: hold the Throwable in x10 (caller-saved scratch). Cannot use x9
    // because emit_store_reg_to_symbol uses x9 internally to materialise the
    // symbol address, which would clobber the value we want to write.
    emitter.instruction(&format!("ldr x10, [x19, #{}]", FIBER_PENDING_THROW_OFFSET)); // x10 = pending Throwable* (NULL if no throw was scheduled)
    emitter.instruction("cbz x10, __rt_fiber_suspend_no_throw");                // skip the raise path when no exception is pending
    emitter.instruction(&format!("str xzr, [x19, #{}]", FIBER_PENDING_THROW_OFFSET)); // clear pending_throw before re-raising so resume() can fire again
    abi::emit_store_reg_to_symbol(emitter, "x10", "_exc_value", 0);             // _exc_value = the Throwable to raise; matches normal `throw` runtime contract
    emitter.instruction("bl __rt_throw_current");                               // unwind into the active try/catch on the fiber's stack (no return)

    emitter.label("__rt_fiber_suspend_no_throw");
    emitter.instruction(&format!("ldr x0, [x19, #{}]", FIBER_TRANSFER_VALUE_OFFSET)); // x0 = fiber->transfer_value.lo (the value the resumer passed)

    emitter.label("__rt_fiber_suspend_done");
    emitter.instruction("ldr x19, [sp]");                                       // restore caller's x19
    emitter.instruction("ldp x29, x30, [sp, #16]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #32");                                     // release the helper frame
    emitter.instruction("ret");                                                 // return the resumer-delivered value
}

/// __rt_fiber_throw: schedule an exception to be raised inside the fiber on
/// resume. The Throwable is parked in `pending_throw`; the resume side of
/// `__rt_fiber_suspend` checks it, clears it, and re-raises via
/// `__rt_throw_current` so the fiber's local try/catch frames see it.
/// Input:  x0 = fiber*, x1 = Throwable*
/// Output: x0 = the value the fiber yields back (or 0 if it terminates without
///         further suspends; the exception itself unwinds inside the fiber).
pub fn emit_fiber_throw(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_throw_x86_64(emitter);
        return;
    }

    emitter.blank();
    emitter.comment("--- runtime: fiber_throw ---");
    emitter.label_global("__rt_fiber_throw");

    emitter.instruction("sub sp, sp, #32");                                     // reserve frame plus saved-x19 slot
    emitter.instruction("stp x29, x30, [sp, #16]");                             // save frame pointer and return address
    emitter.instruction("str x19, [sp]");                                       // preserve caller's x19 across the switch
    emitter.instruction("add x29, sp, #16");                                    // anchor the new frame pointer
    emitter.instruction("mov x19, x0");                                         // x19 = fiber* (callee-saved across the switch)

    // -- guard: throw() requires state == Suspended; otherwise raise FiberError.
    //    Park the Throwable in x20 across the helper because x1 is its argument register.
    emitter.instruction("stp x20, x21, [sp, #-16]!");                           // preserve caller's x20/x21 — both are callee-saved registers we are about to repurpose
    emitter.instruction("mov x20, x1");                                         // x20 = Throwable to deliver, parked across the state check
    emitter.instruction(&format!("ldr x9, [x19, #{}]", FIBER_STATE_OFFSET));    // x9 = receiver fiber state
    emitter.instruction(&format!("cmp x9, #{}", FIBER_STATE_SUSPENDED));        // is the fiber currently paused at a Fiber::suspend() call?
    emitter.instruction("b.eq __rt_fiber_throw_state_ok");                      // proceed only when the fiber is suspended
    abi::emit_symbol_address(emitter, "x0", "_fiber_msg_throw_not_suspended");  // x0 = pointer to the static error message
    emitter.instruction("mov x1, #43");                                         // x1 = error message length in bytes
    emitter.instruction("bl __rt_fiber_throw_state_error");                     // raise FiberError; this call does not return
    emitter.label("__rt_fiber_throw_state_ok");
    emitter.instruction("mov x1, x20");                                         // restore the Throwable into the second argument register
    emitter.instruction("ldp x20, x21, [sp], #16");                             // restore caller's x20/x21 now that the state check is done

    emitter.instruction(&format!("str x1, [x19, #{}]", FIBER_PENDING_THROW_OFFSET)); // fiber->pending_throw = Throwable* to raise on resume
    abi::emit_load_symbol_to_reg(emitter, "x9", "_fiber_current", 0);           // x9 = current execution context
    emitter.instruction(&format!("str x9, [x19, #{}]", FIBER_CALLER_OFFSET));   // fiber->caller = current context

    emitter.instruction("mov x0, x19");                                         // pass fiber* as the switch target
    emitter.instruction("bl __rt_fiber_switch");                                // cooperative switch into the fiber

    // -- check for an escaped exception parked by the trampoline and re-raise it on the caller's stack --
    emitter.instruction(&format!("ldr x9, [x19, #{}]", FIBER_STATE_OFFSET));    // x9 = current fiber state
    emitter.instruction(&format!("cmp x9, #{}", FIBER_STATE_TERMINATED));       // is the fiber terminated?
    emitter.instruction("b.ne __rt_fiber_throw_no_escape");                     // skip the re-raise path when the fiber is still alive
    emitter.instruction(&format!("ldr x10, [x19, #{}]", FIBER_PENDING_THROW_OFFSET)); // x10 = parked Throwable (NULL when termination was clean)
    emitter.instruction("cbz x10, __rt_fiber_throw_no_escape");                 // skip the re-raise path when no exception escaped
    emitter.instruction(&format!("str xzr, [x19, #{}]", FIBER_PENDING_THROW_OFFSET)); // clear pending_throw so subsequent inspections see the clean terminated state
    abi::emit_store_reg_to_symbol(emitter, "x10", "_exc_value", 0);             // _exc_value = the escaped Throwable, ready for __rt_throw_current
    emitter.instruction("bl __rt_throw_current");                               // re-raise on the caller's stack chain (no return)
    emitter.instruction("brk #0xfffe");                                         // defensive trap if __rt_throw_current ever returns
    emitter.label("__rt_fiber_throw_no_escape");

    // -- harvest a suspend value, or PHP null when the fiber terminated cleanly --
    emitter.instruction(&format!("ldr x9, [x19, #{}]", FIBER_STATE_OFFSET));    // x9 = current fiber state after control returned
    emitter.instruction(&format!("cmp x9, #{}", FIBER_STATE_TERMINATED));       // did the fiber finish instead of suspending again?
    emitter.instruction("b.ne __rt_fiber_throw_return_yield");                  // suspended fibers return their next yielded transfer value
    emit_box_null_mixed(emitter);
    emitter.instruction("b __rt_fiber_throw_return_ready");                     // skip the yielded-value load after boxing PHP null
    emitter.label("__rt_fiber_throw_return_yield");
    emitter.instruction(&format!("ldr x0, [x19, #{}]", FIBER_TRANSFER_VALUE_OFFSET)); // x0 = fiber->transfer_value.lo (next yield value)
    emitter.label("__rt_fiber_throw_return_ready");

    emitter.instruction("ldr x19, [sp]");                                       // restore caller's x19
    emitter.instruction("ldp x29, x30, [sp, #16]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #32");                                     // release the helper frame
    emitter.instruction("ret");                                                 // return the value yielded by the fiber
}

pub fn emit_fiber_get_current(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_get_current_x86_64(emitter);
        return;
    }
    emitter.blank();
    emitter.comment("--- runtime: fiber_get_current ---");
    emitter.label_global("__rt_fiber_get_current");
    abi::emit_load_symbol_to_reg(emitter, "x1", "_fiber_current", 0);           // x1 = pointer to the currently running fiber (NULL = main thread)
    emitter.instruction("cbz x1, __rt_fiber_get_current_null");                 // main-thread calls return boxed PHP null
    emitter.instruction("mov x0, #6");                                          // runtime tag 6 = object
    emitter.instruction("mov x2, #0");                                          // object payloads use only the low word
    emitter.instruction("b __rt_mixed_from_value");                             // tail-call the boxer so the caller's link register is preserved
    emitter.label("__rt_fiber_get_current_null");
    emitter.instruction("mov x0, #8");                                          // runtime tag 8 = PHP null
    emitter.instruction("mov x1, #0");                                          // null has no low payload word
    emitter.instruction("mov x2, #0");                                          // null has no high payload word
    emitter.instruction("b __rt_mixed_from_value");                             // tail-call the boxer so the caller's link register is preserved
}

/// __rt_fiber_get_return: read the value a terminated fiber returned.
/// Input:  x0 = fiber*
/// Output: x0 = fiber->transfer_value.lo (set by the entry trampoline at termination)
pub fn emit_fiber_get_return(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_get_return_x86_64(emitter);
        return;
    }
    emitter.blank();
    emitter.comment("--- runtime: fiber_get_return ---");
    emitter.label_global("__rt_fiber_get_return");

    emitter.instruction("sub sp, sp, #32");                                     // reserve frame plus saved-x19 slot
    emitter.instruction("stp x29, x30, [sp, #16]");                             // save frame pointer and return address
    emitter.instruction("str x19, [sp]");                                       // preserve caller's x19 — we use it to remember the receiver across the throw helper
    emitter.instruction("add x29, sp, #16");                                    // anchor the new frame pointer
    emitter.instruction("mov x19, x0");                                         // x19 = receiver fiber* (callee-saved across __rt_fiber_throw_state_error)

    emitter.instruction("cbz x19, __rt_fiber_get_return_null");                 // null fiber pointer is treated as a diagnostic null result

    // -- guard: getReturn() requires state == Terminated; otherwise raise FiberError --
    emitter.instruction(&format!("ldr x9, [x19, #{}]", FIBER_STATE_OFFSET));    // x9 = receiver fiber state
    emitter.instruction(&format!("cmp x9, #{}", FIBER_STATE_TERMINATED));       // has the fiber finished its callable?
    emitter.instruction("b.eq __rt_fiber_get_return_state_ok");                 // proceed only when the fiber has terminated
    abi::emit_symbol_address(emitter, "x0", "_fiber_msg_not_terminated");       // x0 = pointer to the static error message
    emitter.instruction("mov x1, #57");                                         // x1 = error message length in bytes
    emitter.instruction("bl __rt_fiber_throw_state_error");                     // raise FiberError; this call does not return

    emitter.label("__rt_fiber_get_return_state_ok");
    emitter.instruction(&format!("ldr x0, [x19, #{}]", FIBER_TRANSFER_VALUE_OFFSET)); // x0 = fiber->transfer_value.lo (the closure's return value)
    emitter.instruction("b __rt_fiber_get_return_done");                        // skip the NULL-receiver fallback once the return value is loaded

    emitter.label("__rt_fiber_get_return_null");
    emitter.instruction("mov x0, #0");                                          // safe default when a NULL receiver bypassed type checking

    emitter.label("__rt_fiber_get_return_done");
    emitter.instruction("ldr x19, [sp]");                                       // restore caller's x19
    emitter.instruction("ldp x29, x30, [sp, #16]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #32");                                     // release the helper frame
    emitter.instruction("ret");                                                 // hand the captured value back to the caller
}

/// Generic state predicate: `x0 = fiber*`, `x1 = expected state value` → `x0 = 1 or 0`.
pub fn emit_fiber_state_getter(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_state_getter_x86_64(emitter);
        return;
    }
    emitter.blank();
    emitter.comment("--- runtime: fiber_state_eq ---");
    emitter.label_global("__rt_fiber_state_eq");
    emitter.instruction("cbz x0, __rt_fiber_state_eq_false");                   // a NULL fiber pointer never matches any state predicate
    emitter.instruction(&format!("ldr x9, [x0, #{}]", FIBER_STATE_OFFSET));     // x9 = current state stored on the fiber
    emitter.instruction("cmp x9, x1");                                          // compare current state to the requested predicate value
    emitter.instruction("cset x0, eq");                                         // materialise the boolean result (1 when equal, 0 otherwise)
    emitter.instruction("ret");                                                 // return the predicate result
    emitter.label("__rt_fiber_state_eq_false");
    emitter.instruction("mov x0, #0");                                          // NULL fiber pointer always evaluates to false
    emitter.instruction("ret");                                                 // return false to the caller
}

fn emit_throw_state_error_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: fiber_throw_state_error ---");
    emitter.label_global("__rt_fiber_throw_state_error");

    emitter.instruction("push rbp");                                            // preserve the caller frame pointer while building FiberError
    emitter.instruction("mov rbp, rsp");                                        // establish a stable frame base for the throw helper
    emitter.instruction("push r12");                                            // preserve the message pointer across heap allocation
    emitter.instruction("push r13");                                            // preserve the message length across heap allocation
    emitter.instruction("mov r12, rdi");                                        // r12 = message bytes pointer
    emitter.instruction("mov r13, rsi");                                        // r13 = message length

    emitter.instruction("mov rax, 24");                                         // FiberError object size (8 class_id + 16 message property)
    emitter.instruction("call __rt_heap_alloc");                                // rax = freshly allocated payload pointer
    emitter.instruction(&format!("mov r10, 0x{:x}", (X86_64_HEAP_MAGIC_HI32 << 32) | 4)); // materialize the object heap kind word
    emitter.instruction("mov QWORD PTR [rax - 8], r10");                        // stamp the kind in the uniform heap header
    abi::emit_load_symbol_to_reg(emitter, "r10", "_fiber_error_class_id", 0);   // r10 = runtime class id of FiberError
    emitter.instruction("mov QWORD PTR [rax], r10");                            // store FiberError class id at the object header
    emitter.instruction("mov QWORD PTR [rax + 8], r12");                        // message property low half = bytes pointer
    emitter.instruction("mov QWORD PTR [rax + 16], r13");                       // message property high half = byte length
    abi::emit_store_reg_to_symbol(emitter, "rax", "_exc_value", 0);             // _exc_value = the freshly built FiberError
    emitter.instruction("call __rt_throw_current");                             // unwind into the active try/catch chain (no return)
    emitter.instruction("ud2");                                                 // defensive trap: __rt_throw_current must not return here
}

fn emit_construct_x86_64(emitter: &mut Emitter) {
    let initial_frame_bytes = fiber_initial_stack_frame_bytes(emitter.target.arch);
    let initial_entry_offset = fiber_initial_entry_offset(emitter.target.arch);

    emitter.blank();
    emitter.comment("--- runtime: fiber_construct ---");
    emitter.label_global("__rt_fiber_construct");

    // -- frame: keep callable/class/wrapper/object across heap and stack calls --
    emitter.instruction("push rbp");                                            // preserve the caller frame pointer for the constructor helper
    emitter.instruction("mov rbp, rsp");                                        // establish a stable frame base while nested helpers run
    emitter.instruction("push r12");                                            // preserve callable across heap allocation
    emitter.instruction("push r13");                                            // preserve class_id across heap allocation
    emitter.instruction("push r14");                                            // preserve the allocated Fiber object pointer
    emitter.instruction("push r15");                                            // preserve the generated Fiber wrapper pointer
    emitter.instruction("mov r12, rdi");                                        // r12 = callable pointer
    emitter.instruction("mov r13, rsi");                                        // r13 = Fiber class_id
    emitter.instruction("mov r15, rdx");                                        // r15 = generated Fiber wrapper pointer

    // -- allocate the Fiber object payload --
    emitter.instruction(&format!("mov rax, {}", FIBER_OBJECT_SIZE));            // size in bytes for the Fiber object payload
    emitter.instruction("call __rt_heap_alloc");                                // rax = pointer to the object payload
    emitter.instruction("mov r14, rax");                                        // r14 = Fiber object pointer kept until return
    emitter.instruction(&format!("mov r10, 0x{:x}", (X86_64_HEAP_MAGIC_HI32 << 32) | 4)); // materialize the object heap kind word
    emitter.instruction("mov QWORD PTR [r14 - 8], r10");                        // stamp the allocation as an object instance
    emitter.instruction("mov QWORD PTR [r14], r13");                            // store the runtime class_id at the object header

    // -- zero-initialise every Fiber field before populating meaningful ones --
    emitter.instruction("lea r10, [r14 + 8]");                                  // r10 = first runtime-managed Fiber field after class_id
    emitter.instruction(&format!("mov r11, {}", (FIBER_OBJECT_SIZE - 8) / 8));  // r11 = number of qword fields to clear
    emitter.label("__rt_fiber_construct_zero_object_loop");
    emitter.instruction("mov QWORD PTR [r10], 0");                              // clear the current runtime-managed Fiber qword field
    emitter.instruction("add r10, 8");                                          // advance to the next qword field
    emitter.instruction("sub r11, 1");                                          // consume one field from the clear count
    emitter.instruction("jne __rt_fiber_construct_zero_object_loop");           // continue until every runtime-managed field is zeroed

    emitter.instruction(&format!("mov r10, {}", FIBER_START_ARGS_MAX));         // default user_arg_max = full slot count
    emitter.instruction(&format!("mov QWORD PTR [r14 + {}], r10", FIBER_USER_ARG_MAX_OFFSET)); // user_arg_max stored on the freshly built fiber
    emitter.instruction(&format!("mov QWORD PTR [r14 + {}], r12", FIBER_CALLABLE_OFFSET)); // callable.lo = closure pointer
    emitter.instruction(&format!("mov QWORD PTR [r14 + {}], r15", FIBER_CALLABLE_WRAPPER_OFFSET)); // callable wrapper = Fiber entry ABI adapter

    // -- allocate the per-fiber stack via mmap; alloc returns base/top/total --
    emitter.instruction(&format!("mov edi, {}", FIBER_DEFAULT_STACK_SIZE));     // request the default usable fiber stack size in bytes
    emitter.instruction("call __rt_fiber_alloc_stack");                         // rax = stack_base, rdx = stack_top, rcx = total mapped length
    emitter.instruction("test rax, rax");                                       // did mmap return a real stack mapping?
    emitter.instruction("jz __rt_fiber_construct_stack_failed");                // abort construction before writing a fake frame at NULL
    emitter.instruction(&format!("mov QWORD PTR [r14 + {}], rax", FIBER_STACK_BASE_OFFSET)); // stack_base = mmap mapping start
    emitter.instruction(&format!("mov QWORD PTR [r14 + {}], rdx", FIBER_STACK_TOP_OFFSET)); // stack_top = initial SP target
    emitter.instruction(&format!("mov QWORD PTR [r14 + {}], rcx", FIBER_STACK_SIZE_OFFSET)); // stack_size = total mapped length

    // -- carve out and zero a fake initial frame at the top of the stack --
    emitter.instruction(&format!("lea r10, [rdx - {}]", initial_frame_bytes));  // r10 = initial saved_sp for the switch restore path
    emitter.instruction(&format!("mov QWORD PTR [r14 + {}], r10", FIBER_SAVED_SP_OFFSET)); // saved_sp points at the fake switch frame
    emitter.instruction("mov r11, r10");                                        // r11 = cursor through the fake frame
    emitter.instruction(&format!("mov rcx, {}", initial_frame_bytes / 8));      // rcx = number of qwords to zero in the fake frame
    emitter.label("__rt_fiber_construct_zero_frame_loop");
    emitter.instruction("mov QWORD PTR [r11], 0");                              // zero one saved-register or saved-return slot
    emitter.instruction("add r11, 8");                                          // advance to the next fake-frame qword
    emitter.instruction("sub rcx, 1");                                          // consume one fake-frame qword
    emitter.instruction("jne __rt_fiber_construct_zero_frame_loop");            // continue until the fake frame is zeroed
    abi::emit_symbol_address(emitter, "r11", "__rt_fiber_entry");              // r11 = absolute address of the entry trampoline
    emitter.instruction(&format!("mov QWORD PTR [r10 + {}], r11", initial_entry_offset)); // saved return address = entry trampoline

    // -- finish: state = NotStarted and return the new Fiber pointer --
    emitter.instruction(&format!("mov QWORD PTR [r14 + {}], {}", FIBER_STATE_OFFSET, FIBER_STATE_NOT_STARTED)); // state = NotStarted
    emitter.instruction("mov rax, r14");                                        // return the freshly built Fiber pointer
    emitter.instruction("pop r15");                                             // restore caller's r15
    emitter.instruction("pop r14");                                             // restore caller's r14
    emitter.instruction("pop r13");                                             // restore caller's r13
    emitter.instruction("pop r12");                                             // restore caller's r12
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // hand the new Fiber object back to the constructor caller

    emitter.label("__rt_fiber_construct_stack_failed");
    abi::emit_symbol_address(emitter, "rdi", "_fiber_msg_stack_alloc_failed");  // rdi = pointer to the static stack-allocation failure message
    emitter.instruction("mov esi, 27");                                         // rsi = error message length in bytes
    emitter.instruction("call __rt_fiber_throw_state_error");                   // raise FiberError instead of dereferencing a NULL stack top
    emitter.instruction("ud2");                                                 // defensive trap: the throw helper must not return
}

fn emit_start_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: fiber_start ---");
    emitter.label_global("__rt_fiber_start");

    emitter.instruction("push rbp");                                            // preserve the caller frame pointer while switching fibers
    emitter.instruction("mov rbp, rsp");                                        // establish a stable frame base for the start helper
    emitter.instruction("push r12");                                            // preserve the receiver Fiber pointer across the cooperative switch
    emitter.instruction("sub rsp, 8");                                          // keep the SysV stack aligned after saving one callee-saved register
    emitter.instruction("mov r12, rdi");                                        // r12 = fiber object pointer
    emitter.instruction(&format!("mov r10, QWORD PTR [r12 + {}]", FIBER_STATE_OFFSET)); // r10 = receiver fiber state
    emitter.instruction(&format!("cmp r10, {}", FIBER_STATE_NOT_STARTED));      // is the fiber still in the NotStarted state?
    emitter.instruction("je __rt_fiber_start_state_ok");                        // proceed when the fiber has not been started yet
    abi::emit_symbol_address(emitter, "rdi", "_fiber_msg_already_started");     // rdi = pointer to the static error message
    emitter.instruction("mov esi, 50");                                         // rsi = error message length in bytes
    emitter.instruction("call __rt_fiber_throw_state_error");                   // raise FiberError; this call does not return
    emitter.label("__rt_fiber_start_state_ok");
    abi::emit_load_symbol_to_reg(emitter, "r10", "_fiber_current", 0);          // r10 = whoever is running right now
    emitter.instruction(&format!("mov QWORD PTR [r12 + {}], r10", FIBER_CALLER_OFFSET)); // fiber->caller = current execution context
    emitter.instruction("mov rdi, r12");                                        // pass fiber* as the switch target
    emitter.instruction("call __rt_fiber_switch");                              // cooperative context switch into the fiber
    emit_check_escape_x86_64(emitter, "start");
    emitter.instruction(&format!("mov r10, QWORD PTR [r12 + {}]", FIBER_STATE_OFFSET)); // r10 = current fiber state after control returned
    emitter.instruction(&format!("cmp r10, {}", FIBER_STATE_TERMINATED));       // did the fiber finish instead of suspending?
    emitter.instruction("jne __rt_fiber_start_return_yield");                   // suspended fibers return their yielded transfer value
    emit_box_null_mixed(emitter);
    emitter.instruction("jmp __rt_fiber_start_return_ready");                   // skip the yielded-value load after boxing PHP null
    emitter.label("__rt_fiber_start_return_yield");
    emitter.instruction(&format!("mov rax, QWORD PTR [r12 + {}]", FIBER_TRANSFER_VALUE_OFFSET)); // rax = fiber->transfer_value.lo
    emitter.label("__rt_fiber_start_return_ready");
    emitter.instruction("add rsp, 8");                                          // drop the alignment pad
    emitter.instruction("pop r12");                                             // restore caller's r12
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return the harvested value to the caller
}

fn emit_resume_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: fiber_resume ---");
    emitter.label_global("__rt_fiber_resume");

    emitter.instruction("push rbp");                                            // preserve the caller frame pointer while switching fibers
    emitter.instruction("mov rbp, rsp");                                        // establish a stable frame base for the resume helper
    emitter.instruction("push r12");                                            // preserve the receiver Fiber pointer across the cooperative switch
    emitter.instruction("push r13");                                            // preserve the resume value across state validation
    emitter.instruction("mov r12, rdi");                                        // r12 = fiber object pointer
    emitter.instruction("mov r13, rsi");                                        // r13 = boxed Mixed value to deliver
    emitter.instruction(&format!("mov r10, QWORD PTR [r12 + {}]", FIBER_STATE_OFFSET)); // r10 = receiver fiber state
    emitter.instruction(&format!("cmp r10, {}", FIBER_STATE_SUSPENDED));        // is the fiber currently paused at Fiber::suspend()?
    emitter.instruction("je __rt_fiber_resume_state_ok");                       // proceed only when the fiber is suspended
    abi::emit_symbol_address(emitter, "rdi", "_fiber_msg_not_suspended");       // rdi = pointer to the static error message
    emitter.instruction("mov esi, 43");                                         // rsi = error message length in bytes
    emitter.instruction("call __rt_fiber_throw_state_error");                   // raise FiberError; this call does not return
    emitter.label("__rt_fiber_resume_state_ok");
    emitter.instruction(&format!("mov QWORD PTR [r12 + {}], r13", FIBER_TRANSFER_VALUE_OFFSET)); // fiber->transfer_value.lo = resume value
    abi::emit_load_symbol_to_reg(emitter, "r10", "_fiber_current", 0);          // r10 = current execution context to remember as caller
    emitter.instruction(&format!("mov QWORD PTR [r12 + {}], r10", FIBER_CALLER_OFFSET)); // fiber->caller = current execution context
    emitter.instruction("mov rdi, r12");                                        // pass fiber* as the switch target
    emitter.instruction("call __rt_fiber_switch");                              // cooperative context switch into the fiber
    emit_check_escape_x86_64(emitter, "resume");
    emitter.instruction(&format!("mov r10, QWORD PTR [r12 + {}]", FIBER_STATE_OFFSET)); // r10 = current fiber state after control returned
    emitter.instruction(&format!("cmp r10, {}", FIBER_STATE_TERMINATED));       // did the fiber finish instead of suspending again?
    emitter.instruction("jne __rt_fiber_resume_return_yield");                  // suspended fibers return their next yielded transfer value
    emit_box_null_mixed(emitter);
    emitter.instruction("jmp __rt_fiber_resume_return_ready");                  // skip the yielded-value load after boxing PHP null
    emitter.label("__rt_fiber_resume_return_yield");
    emitter.instruction(&format!("mov rax, QWORD PTR [r12 + {}]", FIBER_TRANSFER_VALUE_OFFSET)); // rax = fiber->transfer_value.lo
    emitter.label("__rt_fiber_resume_return_ready");
    emitter.instruction("pop r13");                                             // restore caller's r13
    emitter.instruction("pop r12");                                             // restore caller's r12
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return the harvested value to the caller
}

fn emit_suspend_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: fiber_suspend ---");
    emitter.label_global("__rt_fiber_suspend");

    emitter.instruction("push rbp");                                            // preserve the caller frame pointer while yielding
    emitter.instruction("mov rbp, rsp");                                        // establish a stable frame base for the suspend helper
    emitter.instruction("push r12");                                            // preserve the current Fiber pointer across the cooperative switch
    emitter.instruction("push r13");                                            // preserve the yielded value across state validation
    emitter.instruction("mov r13, rdi");                                        // r13 = boxed Mixed value being yielded
    abi::emit_load_symbol_to_reg(emitter, "r12", "_fiber_current", 0);          // r12 = currently running fiber* (NULL means main)
    emitter.instruction("test r12, r12");                                       // are we executing inside a Fiber?
    emitter.instruction("jne __rt_fiber_suspend_state_ok");                     // proceed when suspend() is called from a Fiber
    abi::emit_symbol_address(emitter, "rdi", "_fiber_msg_suspend_outside");     // rdi = pointer to the static error message
    emitter.instruction("mov esi, 33");                                         // rsi = error message length in bytes
    emitter.instruction("call __rt_fiber_throw_state_error");                   // raise FiberError; this call does not return
    emitter.label("__rt_fiber_suspend_state_ok");
    emitter.instruction(&format!("mov QWORD PTR [r12 + {}], r13", FIBER_TRANSFER_VALUE_OFFSET)); // fiber->transfer_value.lo = yielded value
    emitter.instruction(&format!("mov QWORD PTR [r12 + {}], {}", FIBER_STATE_OFFSET, FIBER_STATE_SUSPENDED)); // fiber->state = Suspended
    emitter.instruction(&format!("mov rdi, QWORD PTR [r12 + {}]", FIBER_CALLER_OFFSET)); // rdi = fiber->caller
    emitter.instruction("call __rt_fiber_switch");                              // hand control back to the caller's resume site
    emitter.instruction(&format!("mov QWORD PTR [r12 + {}], {}", FIBER_STATE_OFFSET, FIBER_STATE_RUNNING)); // fiber->state = Running
    emitter.instruction(&format!("mov r10, QWORD PTR [r12 + {}]", FIBER_PENDING_THROW_OFFSET)); // r10 = pending Throwable*
    emitter.instruction("test r10, r10");                                       // did Fiber->throw() schedule an exception?
    emitter.instruction("je __rt_fiber_suspend_no_throw");                      // skip the raise path when no exception is pending
    emitter.instruction(&format!("mov QWORD PTR [r12 + {}], 0", FIBER_PENDING_THROW_OFFSET)); // clear pending_throw before re-raising
    abi::emit_store_reg_to_symbol(emitter, "r10", "_exc_value", 0);             // _exc_value = Throwable to raise inside this Fiber
    emitter.instruction("call __rt_throw_current");                             // unwind into the active try/catch on the fiber stack
    emitter.label("__rt_fiber_suspend_no_throw");
    emitter.instruction(&format!("mov rax, QWORD PTR [r12 + {}]", FIBER_TRANSFER_VALUE_OFFSET)); // rax = value delivered by resume()
    emitter.instruction("pop r13");                                             // restore caller's r13
    emitter.instruction("pop r12");                                             // restore caller's r12
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return the resumer-delivered value
}

fn emit_throw_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: fiber_throw ---");
    emitter.label_global("__rt_fiber_throw");

    emitter.instruction("push rbp");                                            // preserve the caller frame pointer while switching fibers
    emitter.instruction("mov rbp, rsp");                                        // establish a stable frame base for the throw helper
    emitter.instruction("push r12");                                            // preserve the receiver Fiber pointer across the cooperative switch
    emitter.instruction("push r13");                                            // preserve the Throwable pointer across state validation
    emitter.instruction("mov r12, rdi");                                        // r12 = fiber object pointer
    emitter.instruction("mov r13, rsi");                                        // r13 = Throwable to deliver
    emitter.instruction(&format!("mov r10, QWORD PTR [r12 + {}]", FIBER_STATE_OFFSET)); // r10 = receiver fiber state
    emitter.instruction(&format!("cmp r10, {}", FIBER_STATE_SUSPENDED));        // is the fiber currently paused at Fiber::suspend()?
    emitter.instruction("je __rt_fiber_throw_state_ok");                        // proceed only when the fiber is suspended
    abi::emit_symbol_address(emitter, "rdi", "_fiber_msg_throw_not_suspended"); // rdi = pointer to the static error message
    emitter.instruction("mov esi, 43");                                         // rsi = error message length in bytes
    emitter.instruction("call __rt_fiber_throw_state_error");                   // raise FiberError; this call does not return
    emitter.label("__rt_fiber_throw_state_ok");
    emitter.instruction(&format!("mov QWORD PTR [r12 + {}], r13", FIBER_PENDING_THROW_OFFSET)); // fiber->pending_throw = Throwable*
    abi::emit_load_symbol_to_reg(emitter, "r10", "_fiber_current", 0);          // r10 = current execution context
    emitter.instruction(&format!("mov QWORD PTR [r12 + {}], r10", FIBER_CALLER_OFFSET)); // fiber->caller = current context
    emitter.instruction("mov rdi, r12");                                        // pass fiber* as the switch target
    emitter.instruction("call __rt_fiber_switch");                              // cooperative switch into the fiber
    emit_check_escape_x86_64(emitter, "throw");
    emitter.instruction(&format!("mov r10, QWORD PTR [r12 + {}]", FIBER_STATE_OFFSET)); // r10 = current fiber state after control returned
    emitter.instruction(&format!("cmp r10, {}", FIBER_STATE_TERMINATED));       // did the fiber finish instead of suspending again?
    emitter.instruction("jne __rt_fiber_throw_return_yield");                   // suspended fibers return their next yielded transfer value
    emit_box_null_mixed(emitter);
    emitter.instruction("jmp __rt_fiber_throw_return_ready");                   // skip the yielded-value load after boxing PHP null
    emitter.label("__rt_fiber_throw_return_yield");
    emitter.instruction(&format!("mov rax, QWORD PTR [r12 + {}]", FIBER_TRANSFER_VALUE_OFFSET)); // rax = fiber->transfer_value.lo
    emitter.label("__rt_fiber_throw_return_ready");
    emitter.instruction("pop r13");                                             // restore caller's r13
    emitter.instruction("pop r12");                                             // restore caller's r12
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return the value yielded by the fiber
}

fn emit_get_current_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: fiber_get_current ---");
    emitter.label_global("__rt_fiber_get_current");

    abi::emit_load_symbol_to_reg(emitter, "rdi", "_fiber_current", 0);          // rdi = pointer to the currently running fiber
    emitter.instruction("test rdi, rdi");                                       // is this call running on the main thread?
    emitter.instruction("je __rt_fiber_get_current_null");                      // main-thread calls return boxed PHP null
    emitter.instruction("mov rax, 6");                                          // runtime tag 6 = object
    emitter.instruction("xor esi, esi");                                        // object payloads use only the low word
    emitter.instruction("jmp __rt_mixed_from_value");                           // tail-call the boxer so the caller's return address is preserved
    emitter.label("__rt_fiber_get_current_null");
    emitter.instruction("mov rax, 8");                                          // runtime tag 8 = PHP null
    emitter.instruction("xor edi, edi");                                        // null has no low payload word
    emitter.instruction("xor esi, esi");                                        // null has no high payload word
    emitter.instruction("jmp __rt_mixed_from_value");                           // tail-call the boxer so the caller's return address is preserved
}

fn emit_get_return_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: fiber_get_return ---");
    emitter.label_global("__rt_fiber_get_return");

    emitter.instruction("push rbp");                                            // preserve the caller frame pointer while checking state
    emitter.instruction("mov rbp, rsp");                                        // establish a stable frame base for getReturn
    emitter.instruction("push r12");                                            // preserve the receiver Fiber pointer across a potential throw
    emitter.instruction("sub rsp, 8");                                          // keep the SysV stack aligned after saving one callee-saved register
    emitter.instruction("mov r12, rdi");                                        // r12 = receiver fiber pointer
    emitter.instruction("test r12, r12");                                       // is the receiver defensively NULL?
    emitter.instruction("je __rt_fiber_get_return_null");                       // null fiber pointers return a safe default
    emitter.instruction(&format!("mov r10, QWORD PTR [r12 + {}]", FIBER_STATE_OFFSET)); // r10 = receiver fiber state
    emitter.instruction(&format!("cmp r10, {}", FIBER_STATE_TERMINATED));       // has the fiber finished its callable?
    emitter.instruction("je __rt_fiber_get_return_state_ok");                   // proceed only when the fiber has terminated
    abi::emit_symbol_address(emitter, "rdi", "_fiber_msg_not_terminated");      // rdi = pointer to the static error message
    emitter.instruction("mov esi, 57");                                         // rsi = error message length in bytes
    emitter.instruction("call __rt_fiber_throw_state_error");                   // raise FiberError; this call does not return
    emitter.label("__rt_fiber_get_return_state_ok");
    emitter.instruction(&format!("mov rax, QWORD PTR [r12 + {}]", FIBER_TRANSFER_VALUE_OFFSET)); // rax = fiber->transfer_value.lo
    emitter.instruction("jmp __rt_fiber_get_return_done");                      // skip the NULL-receiver fallback once the value is loaded
    emitter.label("__rt_fiber_get_return_null");
    emitter.instruction("xor eax, eax");                                        // safe default when a NULL receiver bypassed type checking
    emitter.label("__rt_fiber_get_return_done");
    emitter.instruction("add rsp, 8");                                          // drop the alignment pad
    emitter.instruction("pop r12");                                             // restore caller's r12
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // hand the captured value back to the caller
}

fn emit_state_getter_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: fiber_state_eq ---");
    emitter.label_global("__rt_fiber_state_eq");

    emitter.instruction("test rdi, rdi");                                       // a NULL fiber pointer never matches any state predicate
    emitter.instruction("je __rt_fiber_state_eq_false");                        // return false for NULL receivers
    emitter.instruction(&format!("mov r10, QWORD PTR [rdi + {}]", FIBER_STATE_OFFSET)); // r10 = current state stored on the fiber
    emitter.instruction("cmp r10, rsi");                                        // compare current state to the requested predicate value
    emitter.instruction("sete al");                                             // materialize the boolean result in the low result byte
    emitter.instruction("movzx eax, al");                                       // widen the boolean result to the canonical integer register
    emitter.instruction("ret");                                                 // return the predicate result
    emitter.label("__rt_fiber_state_eq_false");
    emitter.instruction("xor eax, eax");                                        // NULL fiber pointer always evaluates to false
    emitter.instruction("ret");                                                 // return false to the caller
}

fn emit_check_escape_x86_64(emitter: &mut Emitter, prefix: &str) {
    emitter.instruction(&format!("mov r10, QWORD PTR [r12 + {}]", FIBER_STATE_OFFSET)); // r10 = current fiber state
    emitter.instruction(&format!("cmp r10, {}", FIBER_STATE_TERMINATED));       // is the fiber terminated?
    emitter.instruction(&format!("jne __rt_fiber_{}_no_escape", prefix));       // skip re-raise when the fiber is still alive
    emitter.instruction(&format!("mov r11, QWORD PTR [r12 + {}]", FIBER_PENDING_THROW_OFFSET)); // r11 = parked Throwable
    emitter.instruction("test r11, r11");                                       // did an exception escape from the fiber entry boundary?
    emitter.instruction(&format!("je __rt_fiber_{}_no_escape", prefix));        // skip re-raise when termination was clean
    emitter.instruction(&format!("mov QWORD PTR [r12 + {}], 0", FIBER_PENDING_THROW_OFFSET)); // clear pending_throw before re-raising
    abi::emit_store_reg_to_symbol(emitter, "r11", "_exc_value", 0);             // _exc_value = escaped Throwable ready for __rt_throw_current
    emitter.instruction("call __rt_throw_current");                             // re-raise on the caller's stack chain
    emitter.instruction("ud2");                                                 // defensive trap if __rt_throw_current ever returns
    emitter.label(&format!("__rt_fiber_{}_no_escape", prefix));
}

fn emit_box_null_mixed(emitter: &mut Emitter) {
    match emitter.target.arch {
        Arch::AArch64 => {
            emitter.instruction("mov x0, #8");                                  // runtime tag 8 = PHP null
            emitter.instruction("mov x1, #0");                                  // null has no low payload word
            emitter.instruction("mov x2, #0");                                  // null has no high payload word
            emitter.instruction("bl __rt_mixed_from_value");                    // allocate a boxed Mixed null cell for the PHP-visible result
        }
        Arch::X86_64 => {
            emitter.instruction("mov rax, 8");                                  // runtime tag 8 = PHP null
            emitter.instruction("xor edi, edi");                                // null has no low payload word
            emitter.instruction("xor esi, esi");                                // null has no high payload word
            emitter.instruction("call __rt_mixed_from_value");                  // allocate a boxed Mixed null cell for the PHP-visible result
        }
    }
}
