//! Purpose:
//! Emits the `_fn_<f>` wrapper symbol for a generator function: allocates a
//! fresh `GeneratorFrame` on the heap, stamps it with the Generator class id
//! and the resume function address, copies integer parameters into their
//! frame slots, zeroes locals, and returns the frame pointer.
//!
//! Called from:
//!  - `crate::codegen::functions::generator::emit_generator_function()` via
//!    the parent module's `emit_wrapper` re-export.
//!
//! Key details:
//!  - Wrapper layout: 16 bytes for `x29`/`x30` save plus a 16-byte-aligned
//!    parameter stash; the heap frame itself is `aligned_frame_size_with_slots`.
//!  - All fixed-header slots (`class_id`, `resume_fn`, `state_idx`/`flags`,
//!    `auto_key_counter`, key/value/return/sent pointers, `delegated_iter`,
//!    `layout_id`) are initialised here so the resume function and runtime
//!    helpers can rely on the invariants on first entry.

use super::{aligned_frame_size_with_slots, slot_offset};
use crate::codegen::emit::Emitter;
use crate::codegen::runtime::generators::frame as gen_frame;

pub(in crate::codegen::functions::generator) fn emit_wrapper(
    emitter: &mut Emitter,
    label: &str,
    resume_label: &str,
    class_id: u64,
    int_param_count: usize,
    int_local_count: usize,
) {
    let total_slots = int_param_count + int_local_count;
    let frame_size = aligned_frame_size_with_slots(total_slots);

    emitter.blank();
    emitter.comment(&format!("--- generator wrapper {} ---", label));
    emitter.label_global(label);

    let param_save_bytes = if int_param_count > 0 {
        (int_param_count * 8 + 15) & !15
    } else {
        0
    };
    let prologue_bytes = 16 + param_save_bytes;
    emitter.instruction(&format!("sub sp, sp, #{}", prologue_bytes));           // reserve the wrapper's stack frame
    emitter.instruction(&format!("stp x29, x30, [sp, #{}]", param_save_bytes)); // save frame pointer and return address above the param stash
    emitter.instruction(&format!("add x29, sp, #{}", param_save_bytes));        // establish the wrapper's frame pointer

    for i in 0..int_param_count {
        emitter.instruction(&format!("str x{}, [sp, #{}]", i, i * 8));          // park parameter i in its stash slot
    }

    emitter.instruction(&format!("mov x0, #{}", frame_size));                   // total frame size including parameter and local slots
    emitter.instruction("bl __rt_heap_alloc");                                  // x0 = pointer to fresh GeneratorFrame

    emitter.instruction("mov x9, #4");                                          // heap kind 4 = object instance
    emitter.instruction("str x9, [x0, #-8]");                                   // write kind into the uniform heap header

    emitter.instruction(&format!("mov x9, #{}", class_id));                     // load Generator's compile-time class id
    emitter.instruction(&format!("str x9, [x0, #{}]", gen_frame::OFF_CLASS_ID)); // class_id at OFF_CLASS_ID

    emitter.instruction(&format!("adr x9, {}", resume_label));                  // load address of the resume function symbol
    emitter.instruction(&format!("str x9, [x0, #{}]", gen_frame::OFF_RESUME_FN)); // resume_fn at OFF_RESUME_FN

    emitter.instruction(&format!("str xzr, [x0, #{}]", gen_frame::OFF_STATE_IDX));        // state_idx + flags
    emitter.instruction(&format!("str xzr, [x0, #{}]", gen_frame::OFF_AUTO_KEY_COUNTER)); // auto_key_counter
    emitter.instruction(&format!("str xzr, [x0, #{}]", gen_frame::OFF_LAST_KEY));         // last_key (Mixed pointer, NULL until first yield)
    emitter.instruction(&format!("str xzr, [x0, #{}]", gen_frame::OFF_LAST_VALUE));       // last_value (Mixed pointer)
    emitter.instruction(&format!("str xzr, [x0, #{}]", gen_frame::OFF_RETURN_VALUE));     // return_value (Mixed pointer)
    emitter.instruction(&format!("str xzr, [x0, #{}]", gen_frame::OFF_SENT_VALUE));       // sent_value (Mixed pointer)
    emitter.instruction(&format!("str xzr, [x0, #{}]", gen_frame::OFF_DELEGATED_ITER));   // delegated_iter
    emitter.instruction(&format!("str xzr, [x0, #{}]", gen_frame::OFF_LAYOUT_ID));        // layout_id

    for i in 0..int_param_count {
        let frame_off = slot_offset(i);
        emitter.instruction(&format!("ldr x9, [sp, #{}]", i * 8));              // reload saved parameter i from the stash
        emitter.instruction(&format!("str x9, [x0, #{}]", frame_off));          // store parameter i in its frame slot
    }

    for i in 0..int_local_count {
        let frame_off = slot_offset(int_param_count + i);
        emitter.instruction(&format!("str xzr, [x0, #{}]", frame_off));         // zero-initialize local i's frame slot
    }

    emitter.instruction(&format!("ldp x29, x30, [sp, #{}]", param_save_bytes)); // restore frame pointer and return address
    emitter.instruction(&format!("add sp, sp, #{}", prologue_bytes));           // release the wrapper's stack frame
    emitter.instruction("ret");                                                 // return the frame pointer
}
