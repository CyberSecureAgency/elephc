//! Fiber runtime — cooperative coroutines (PHP 8.1 Fiber semantics).
//!
//! Object layout (allocated via __rt_heap_alloc, "object instance" kind):
//!
//! | Offset | Size | Field             | Notes                                 |
//! |--------|------|-------------------|---------------------------------------|
//! | -8     | 8    | heap kind = 4     | written by allocator                  |
//! | 0      | 8    | class_id          | runtime class id of `Fiber`           |
//! | 8      | 8    | state             | 0=NotStarted 1=Running 2=Suspended 3=Terminated |
//! | 16     | 8    | stack_base        | low address of fiber stack (heap ptr) |
//! | 24     | 8    | stack_top         | high address (initial SP, 16-aligned) |
//! | 32     | 8    | stack_size        | total bytes of the stack region       |
//! | 40     | 8    | saved_sp          | SP saved when fiber is not running    |
//! | 48     | 16   | callable          | closure pointer + optional entry wrapper |
//! | 64     | 8    | caller            | Fiber* of resumer (NULL = main)       |
//! | 72     | 16   | transfer_value    | mixed cell — value in transit         |
//! | 88     | 8    | pending_throw     | Throwable* to rethrow on resume       |
//! | 96     | 8    | own_exc_head      | saved _exc_handler_top for this fiber |
//! | 104    | 8    | own_call_frame    | saved _exc_call_frame_top for this fiber |
//! | 112    | 56   | start_args[0..7]  | up to 7 Mixed pointers passed to start() (one per AArch64 int arg-reg minus $this) |
//! | 168    | 8    | user_arg_max      | how many start_args slots `start()` may write — leaves trailing slots untouched so `new Fiber(use(...))` captures survive |
//! | 176    | 56   | float_args[0..7]  | parallel slot file for float captures (loaded into d0..d6 by the trampoline) |
//!
//! Total payload = 232 bytes.

mod alloc;
mod api;
mod entry;
mod switch;

pub(crate) use alloc::{emit_fiber_alloc_stack, emit_fiber_free_stack};
pub(crate) use api::{
    emit_fiber_construct, emit_fiber_get_current, emit_fiber_get_return, emit_fiber_resume,
    emit_fiber_start, emit_fiber_state_getter, emit_fiber_suspend, emit_fiber_throw,
    emit_fiber_throw_state_error,
};
pub(crate) use entry::{emit_fiber_entry, emit_fiber_invoke_callable_stub};
pub(crate) use switch::emit_fiber_switch;

// ── Fiber object field offsets ───────────────────────────────────────
pub(crate) const FIBER_STATE_OFFSET: i32 = 8;
pub(crate) const FIBER_STACK_BASE_OFFSET: i32 = 16;
pub(crate) const FIBER_STACK_TOP_OFFSET: i32 = 24;
pub(crate) const FIBER_STACK_SIZE_OFFSET: i32 = 32;
pub(crate) const FIBER_SAVED_SP_OFFSET: i32 = 40;
pub(crate) const FIBER_CALLABLE_OFFSET: i32 = 48;
pub(crate) const FIBER_CALLER_OFFSET: i32 = 64;
pub(crate) const FIBER_TRANSFER_VALUE_OFFSET: i32 = 72;
pub(crate) const FIBER_PENDING_THROW_OFFSET: i32 = 88;
pub(crate) const FIBER_OWN_EXC_HEAD_OFFSET: i32 = 96;
pub(crate) const FIBER_OWN_CALL_FRAME_OFFSET: i32 = 104;
pub(crate) const FIBER_START_ARGS_OFFSET: i32 = 112;
pub(crate) const FIBER_START_ARGS_MAX: i32 = 7;
pub(crate) const FIBER_USER_ARG_MAX_OFFSET: i32 = 168;
pub(crate) const FIBER_FLOAT_ARGS_OFFSET: i32 = 176;
pub(crate) const FIBER_FLOAT_ARGS_MAX: i32 = 7;

pub(crate) const FIBER_OBJECT_SIZE: i32 = 232;

// ── Lifecycle states (stored in FIBER_STATE_OFFSET) ──────────────────
// Phase 3 (suspend) will introduce the first user of FIBER_STATE_SUSPENDED.
pub(crate) const FIBER_STATE_NOT_STARTED: i32 = 0;
pub(crate) const FIBER_STATE_RUNNING: i32 = 1;
#[allow(dead_code)]
pub(crate) const FIBER_STATE_SUSPENDED: i32 = 2;
pub(crate) const FIBER_STATE_TERMINATED: i32 = 3;

// ── Default per-fiber stack size ─────────────────────────────────────
pub(crate) const FIBER_DEFAULT_STACK_SIZE: i32 = 256 * 1024;
