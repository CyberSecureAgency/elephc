//! Purpose:
//! Emits runtime helpers for `SplDoublyLinkedList`, `SplStack`, and `SplQueue`.
//! The helpers back PHP-visible mutation, iteration, count, and ArrayAccess methods.
//!
//! Called from:
//! - `crate::codegen::runtime::spl::emit_doubly_linked_list_runtime()`.
//!
//! Key details:
//! - The object stores a class id, an owned mixed-cell indexed array, iterator index, and iterator mode.
//! - Mutating methods take ownership of boxed `Mixed` arguments prepared by call lowering.

use crate::codegen::emit::Emitter;
use crate::codegen::platform::Arch;

use super::{SPL_DLL_ITER_INDEX_OFFSET, SPL_DLL_ITER_MODE_OFFSET, SPL_DLL_STORAGE_OFFSET};

const SPL_DLL_OBJECT_SIZE: i64 = 32;
const SPL_DLL_INITIAL_CAPACITY: i64 = 4;
const NULL_TAG: i64 = 8;
const INT_TAG: i64 = 0;
const ITER_MODE_DELETE: i64 = 1;
const ITER_MODE_LIFO: i64 = 2;
const X86_64_HEAP_MAGIC_HI32: u64 = 0x454C5048;

pub(crate) fn emit_doubly_linked_list_runtime(emitter: &mut Emitter) {
    if emitter.target.arch == Arch::X86_64 {
        emit_x86_64(emitter);
    } else {
        emit_aarch64(emitter);
    }
}

fn emit_aarch64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: spl doubly linked list ---");
    emit_new_aarch64(emitter);
    emit_count_aarch64(emitter);
    emit_is_empty_aarch64(emitter);
    emit_push_aarch64(emitter);
    emit_pop_aarch64(emitter);
    emit_shift_aarch64(emitter);
    emit_unshift_aarch64(emitter);
    emit_insert_aarch64(emitter);
    emit_top_aarch64(emitter);
    emit_bottom_aarch64(emitter);
    emit_iterator_mode_aarch64(emitter);
    emit_rewind_aarch64(emitter);
    emit_next_prev_aarch64(emitter);
    emit_valid_aarch64(emitter);
    emit_current_aarch64(emitter);
    emit_key_aarch64(emitter);
    emit_offset_exists_aarch64(emitter);
    emit_offset_get_aarch64(emitter);
    emit_offset_set_aarch64(emitter);
    emit_offset_unset_aarch64(emitter);
}

fn emit_new_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_new");
    emitter.instruction("sub sp, sp, #32");                                     // reserve constructor spill slots
    emitter.instruction("stp x29, x30, [sp, #16]");                             // save frame pointer and return address
    emitter.instruction("add x29, sp, #16");                                    // establish a frame for nested allocator calls
    emitter.instruction("str x0, [sp, #0]");                                    // save the concrete SPL class id
    emitter.instruction(&format!("mov x0, #{}", SPL_DLL_OBJECT_SIZE));          // request the fixed SPL list object payload size
    emitter.instruction("bl __rt_heap_alloc");                                  // allocate the SPL list object payload
    emitter.instruction("mov x9, #4");                                          // heap kind 4 = object instance
    emitter.instruction("str x9, [x0, #-8]");                                   // stamp the allocation as an object instance
    emitter.instruction("ldr x9, [sp, #0]");                                    // reload the concrete SPL class id
    emitter.instruction("str x9, [x0]");                                        // store the class id at the object header
    emitter.instruction("str x0, [sp, #8]");                                    // save the object pointer while allocating storage
    emitter.instruction(&format!("mov x0, #{}", SPL_DLL_INITIAL_CAPACITY));     // initial internal storage capacity
    emitter.instruction("mov x1, #8");                                          // each internal storage slot holds one Mixed pointer
    emitter.instruction("bl __rt_array_new");                                   // allocate the owned internal mixed-pointer storage
    emitter.instruction("ldr x9, [x0, #-8]");                                   // load the internal array packed kind word
    emitter.instruction("mov x10, #0x700");                                     // runtime value_type tag 7 = boxed Mixed cells
    emitter.instruction("orr x9, x9, x10");                                     // mark internal storage as an array of Mixed cells
    emitter.instruction("str x9, [x0, #-8]");                                   // persist the Mixed value_type tag on internal storage
    emitter.instruction("ldr x9, [sp, #8]");                                    // reload the object pointer
    emitter.instruction(&format!("str x0, [x9, #{}]", SPL_DLL_STORAGE_OFFSET)); // object.storage = internal Mixed array
    emitter.instruction(&format!("str xzr, [x9, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // iterator index starts at zero
    emitter.instruction(&format!("str xzr, [x9, #{}]", SPL_DLL_ITER_MODE_OFFSET)); // iterator mode starts FIFO/KEEP
    emitter.instruction("mov x0, x9");                                          // return the initialized SPL object
    emitter.instruction("ldp x29, x30, [sp, #16]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #32");                                     // release constructor spill slots
    emitter.instruction("ret");                                                 // return the object pointer
}

fn emit_count_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_count");
    emitter.instruction(&format!("ldr x9, [x0, #{}]", SPL_DLL_STORAGE_OFFSET)); // load the internal storage array
    emitter.instruction("ldr x0, [x9]");                                        // return the internal storage length
    emitter.instruction("ret");                                                 // return count
}

fn emit_is_empty_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_is_empty");
    emitter.instruction(&format!("ldr x9, [x0, #{}]", SPL_DLL_STORAGE_OFFSET)); // load the internal storage array
    emitter.instruction("ldr x9, [x9]");                                        // read the storage length
    emitter.instruction("cmp x9, #0");                                          // compare length with zero
    emitter.instruction("cset x0, eq");                                         // return true when the list is empty
    emitter.instruction("ret");                                                 // return boolean result
}

fn emit_push_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_push");
    emitter.instruction("sub sp, sp, #32");                                     // reserve spill slots for receiver and return address
    emitter.instruction("stp x29, x30, [sp, #16]");                             // save frame pointer and return address
    emitter.instruction("add x29, sp, #16");                                    // establish a frame for the nested array append
    emitter.instruction("str x0, [sp, #0]");                                    // save receiver while appending to internal storage
    emitter.instruction(&format!("ldr x0, [x0, #{}]", SPL_DLL_STORAGE_OFFSET)); // pass internal storage as array_push receiver
    emitter.instruction("bl __rt_array_push_int");                              // append the owned Mixed pointer without retaining it again
    emitter.instruction("ldr x9, [sp, #0]");                                    // reload receiver after append
    emitter.instruction(&format!("str x0, [x9, #{}]", SPL_DLL_STORAGE_OFFSET)); // store possibly-grown internal storage
    emitter.instruction("ldp x29, x30, [sp, #16]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #32");                                     // release spill slots
    emitter.instruction("ret");                                                 // return void
}

fn emit_pop_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_pop");
    emitter.instruction(&format!("ldr x9, [x0, #{}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("ldr x10, [x9]");                                       // read current storage length
    emitter.instruction("cbz x10, __rt_spl_dll_pop_null");                      // empty list pops to null for now
    emitter.instruction("sub x10, x10, #1");                                    // compute the last occupied index
    emitter.instruction("str x10, [x9]");                                       // shrink storage length by one
    emitter.instruction("add x11, x9, #24");                                    // point at the first storage element
    emitter.instruction("ldr x0, [x11, x10, lsl #3]");                          // return the removed Mixed cell, transferring ownership
    emitter.instruction("str xzr, [x11, x10, lsl #3]");                         // clear the stale slot beyond the new length
    emitter.instruction("ret");                                                 // return removed Mixed cell
    emitter.label("__rt_spl_dll_pop_null");
    emit_tail_boxed_null_aarch64(emitter);
}

fn emit_shift_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_shift");
    emitter.instruction(&format!("ldr x9, [x0, #{}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("ldr x10, [x9]");                                       // read current storage length
    emitter.instruction("cbz x10, __rt_spl_dll_shift_null");                    // empty list shifts to null for now
    emitter.instruction("add x11, x9, #24");                                    // point at the first storage element
    emitter.instruction("ldr x0, [x11]");                                       // capture the removed first Mixed cell
    emitter.instruction("mov x12, #1");                                         // start shifting from element index 1
    emitter.label("__rt_spl_dll_shift_loop");
    emitter.instruction("cmp x12, x10");                                        // have all live elements been shifted left?
    emitter.instruction("b.ge __rt_spl_dll_shift_done");                        // finish once the cursor reaches the old length
    emitter.instruction("ldr x13, [x11, x12, lsl #3]");                         // load the next Mixed pointer
    emitter.instruction("sub x14, x12, #1");                                    // compute the destination index one slot earlier
    emitter.instruction("str x13, [x11, x14, lsl #3]");                         // move the Mixed pointer down by one slot
    emitter.instruction("add x12, x12, #1");                                    // advance the shift cursor
    emitter.instruction("b __rt_spl_dll_shift_loop");                           // continue compacting storage
    emitter.label("__rt_spl_dll_shift_done");
    emitter.instruction("sub x10, x10, #1");                                    // compute the new storage length
    emitter.instruction("str x10, [x9]");                                       // persist the shortened length
    emitter.instruction("str xzr, [x11, x10, lsl #3]");                         // clear the stale tail slot
    emitter.instruction("ret");                                                 // return removed Mixed cell
    emitter.label("__rt_spl_dll_shift_null");
    emit_tail_boxed_null_aarch64(emitter);
}

fn emit_unshift_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_unshift");
    emitter.instruction("mov x2, x1");                                          // move value to the insert helper's value argument
    emitter.instruction("mov x1, #0");                                          // unshift inserts at index zero
    emitter.instruction("b __rt_spl_dll_insert");                               // tail-call the shared insertion helper
}

fn emit_insert_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_insert");
    emitter.instruction("sub sp, sp, #64");                                     // reserve insertion state and call frame
    emitter.instruction("stp x29, x30, [sp, #48]");                             // save frame pointer and return address
    emitter.instruction("add x29, sp, #48");                                    // establish a frame for array growth
    emitter.instruction("str x0, [sp, #0]");                                    // save receiver
    emitter.instruction("str x1, [sp, #8]");                                    // save requested insertion index
    emitter.instruction("str x2, [sp, #16]");                                   // save owned Mixed value to insert
    emitter.instruction("cmp x1, #0");                                          // is the requested index negative?
    emitter.instruction("b.ge __rt_spl_dll_insert_index_nonnegative");          // keep non-negative indexes
    emitter.instruction("str xzr, [sp, #8]");                                   // clamp negative indexes to the beginning
    emitter.label("__rt_spl_dll_insert_index_nonnegative");
    emitter.instruction(&format!("ldr x9, [x0, #{}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("str x9, [sp, #24]");                                   // save current storage pointer
    emitter.instruction("ldr x10, [x9]");                                       // read current storage length
    emitter.instruction("ldr x11, [x9, #8]");                                   // read current storage capacity
    emitter.instruction("ldr x12, [sp, #8]");                                   // reload requested insertion index
    emitter.instruction("cmp x12, x10");                                        // is the index past the end?
    emitter.instruction("b.le __rt_spl_dll_insert_index_in_range");             // keep indexes within the append boundary
    emitter.instruction("str x10, [sp, #8]");                                   // clamp indexes past the end to append
    emitter.label("__rt_spl_dll_insert_index_in_range");
    emitter.instruction("cmp x10, x11");                                        // is storage full?
    emitter.instruction("b.ne __rt_spl_dll_insert_have_capacity");              // skip growth when capacity remains
    emitter.instruction("mov x0, x9");                                          // pass current storage to array_grow
    emitter.instruction("bl __rt_array_grow");                                  // grow internal storage
    emitter.instruction("ldr x9, [sp, #0]");                                    // reload receiver after growth
    emitter.instruction(&format!("str x0, [x9, #{}]", SPL_DLL_STORAGE_OFFSET)); // store possibly-grown internal storage
    emitter.instruction("str x0, [sp, #24]");                                   // save grown storage for insertion
    emitter.label("__rt_spl_dll_insert_have_capacity");
    emitter.instruction("ldr x9, [sp, #24]");                                   // reload storage pointer
    emitter.instruction("ldr x10, [x9]");                                       // reload current length
    emitter.instruction("ldr x12, [sp, #8]");                                   // reload clamped insertion index
    emitter.instruction("add x11, x9, #24");                                    // point at the first storage element
    emitter.instruction("mov x13, x10");                                        // start right-shift cursor at the old length
    emitter.label("__rt_spl_dll_insert_shift_loop");
    emitter.instruction("cmp x13, x12");                                        // has the cursor reached the insertion index?
    emitter.instruction("b.le __rt_spl_dll_insert_store");                      // stop once the insertion slot is free
    emitter.instruction("sub x14, x13, #1");                                    // source index is one slot before the cursor
    emitter.instruction("ldr x15, [x11, x14, lsl #3]");                         // load the Mixed pointer being shifted right
    emitter.instruction("str x15, [x11, x13, lsl #3]");                         // store the Mixed pointer one slot to the right
    emitter.instruction("sub x13, x13, #1");                                    // move the shift cursor left
    emitter.instruction("b __rt_spl_dll_insert_shift_loop");                    // continue shifting until the insert slot is open
    emitter.label("__rt_spl_dll_insert_store");
    emitter.instruction("ldr x14, [sp, #16]");                                  // reload owned Mixed value to insert
    emitter.instruction("str x14, [x11, x12, lsl #3]");                         // store the owned Mixed value in the insertion slot
    emitter.instruction("add x10, x10, #1");                                    // increase storage length
    emitter.instruction("str x10, [x9]");                                       // persist new storage length
    emitter.instruction("ldp x29, x30, [sp, #48]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #64");                                     // release insertion state
    emitter.instruction("ret");                                                 // return void
}

fn emit_top_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_top");
    emit_peek_index_aarch64(emitter, "__rt_spl_dll_top_null", true);
}

fn emit_bottom_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_bottom");
    emit_peek_index_aarch64(emitter, "__rt_spl_dll_bottom_null", false);
}

fn emit_peek_index_aarch64(emitter: &mut Emitter, null_label: &str, last: bool) {
    emitter.instruction("sub sp, sp, #32");                                     // reserve frame for the incref call
    emitter.instruction("stp x29, x30, [sp, #16]");                             // save frame pointer and return address
    emitter.instruction("add x29, sp, #16");                                    // establish a frame for nested incref
    emitter.instruction(&format!("ldr x9, [x0, #{}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("ldr x10, [x9]");                                       // read storage length
    emitter.instruction(&format!("cbz x10, {}", null_label));                   // empty storage returns null
    if last {
        emitter.instruction("sub x10, x10, #1");                                // choose the last occupied index
    } else {
        emitter.instruction("mov x10, #0");                                     // choose the first occupied index
    }
    emitter.instruction("add x11, x9, #24");                                    // point at the first storage element
    emitter.instruction("ldr x0, [x11, x10, lsl #3]");                          // load the selected Mixed cell
    emitter.instruction("bl __rt_incref");                                      // retain the Mixed cell for the caller while storage keeps its owner
    emitter.instruction("ldp x29, x30, [sp, #16]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #32");                                     // release frame
    emitter.instruction("ret");                                                 // return retained Mixed cell
    emitter.label(null_label);
    emitter.instruction("ldp x29, x30, [sp, #16]");                             // restore frame pointer before null tail-call
    emitter.instruction("add sp, sp, #32");                                     // release frame before null tail-call
    emit_tail_boxed_null_aarch64(emitter);
}

fn emit_iterator_mode_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_set_iterator_mode");
    emitter.instruction(&format!("str x1, [x0, #{}]", SPL_DLL_ITER_MODE_OFFSET)); // store iterator mode bits on the receiver
    emitter.instruction("ret");                                                 // return void
    emitter.label_global("__rt_spl_dll_get_iterator_mode");
    emitter.instruction(&format!("ldr x0, [x0, #{}]", SPL_DLL_ITER_MODE_OFFSET)); // return iterator mode bits
    emitter.instruction("ret");                                                 // return integer mode
}

fn emit_rewind_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_rewind");
    emitter.instruction(&format!("ldr x9, [x0, #{}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("ldr x10, [x9]");                                       // read storage length
    emitter.instruction("cbz x10, __rt_spl_dll_rewind_empty");                  // empty storage rewinds to zero
    emitter.instruction(&format!("ldr x11, [x0, #{}]", SPL_DLL_ITER_MODE_OFFSET)); // load iterator mode bits
    emitter.instruction(&format!("tst x11, #{}", ITER_MODE_LIFO));              // is LIFO traversal requested?
    emitter.instruction("b.eq __rt_spl_dll_rewind_fifo");                       // FIFO traversal starts at index zero
    emitter.instruction("sub x10, x10, #1");                                    // LIFO traversal starts at the last element
    emitter.instruction(&format!("str x10, [x0, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // store starting LIFO index
    emitter.instruction("ret");                                                 // return void
    emitter.label("__rt_spl_dll_rewind_fifo");
    emitter.instruction(&format!("str xzr, [x0, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // store starting FIFO index
    emitter.instruction("ret");                                                 // return void
    emitter.label("__rt_spl_dll_rewind_empty");
    emitter.instruction(&format!("str xzr, [x0, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // reset empty iterators to index zero
    emitter.instruction("ret");                                                 // return void
}

fn emit_next_prev_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_next");
    emit_iterator_step_aarch64(emitter, true);
    emitter.label_global("__rt_spl_dll_prev");
    emit_iterator_step_aarch64(emitter, false);
}

fn emit_iterator_step_aarch64(emitter: &mut Emitter, forward: bool) {
    let fifo_label = if forward {
        "__rt_spl_dll_next_fifo"
    } else {
        "__rt_spl_dll_prev_fifo"
    };
    let delete_label = if forward {
        "__rt_spl_dll_next_delete"
    } else {
        ""
    };
    let done_label = if forward {
        "__rt_spl_dll_next_done"
    } else {
        "__rt_spl_dll_prev_done"
    };
    emitter.instruction(&format!("ldr x9, [x0, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // load current iterator index
    emitter.instruction(&format!("ldr x10, [x0, #{}]", SPL_DLL_ITER_MODE_OFFSET)); // load iterator mode bits
    if forward {
        emitter.instruction(&format!("tst x10, #{}", ITER_MODE_DELETE));        // does next() need to delete the current element?
        emitter.instruction(&format!("b.ne {}", delete_label));                 // delete-mode foreach advances by removing the current slot
    }
    emitter.instruction(&format!("tst x10, #{}", ITER_MODE_LIFO));              // is traversal currently LIFO?
    emitter.instruction(&format!("b.eq {}", fifo_label));                       // FIFO and LIFO move in opposite directions
    if forward {
        emitter.instruction("cbz x9, __rt_spl_dll_next_lifo_exhaust");          // moving forward in LIFO from zero exhausts the iterator
        emitter.instruction("sub x9, x9, #1");                                  // otherwise move to the previous numeric index
        emitter.instruction(&format!("str x9, [x0, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // persist decremented LIFO iterator index
        emitter.instruction("ret");                                             // return void
        emitter.label("__rt_spl_dll_next_lifo_exhaust");
        emitter.instruction(&format!("ldr x10, [x0, #{}]", SPL_DLL_STORAGE_OFFSET)); // load storage to compute exhausted sentinel
        emitter.instruction("ldr x10, [x10]");                                  // storage length is the invalid sentinel
        emitter.instruction(&format!("str x10, [x0, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // store exhausted LIFO sentinel
        emitter.instruction("ret");                                             // return void
    } else {
        emitter.instruction("add x9, x9, #1");                                  // moving prev in LIFO increases the numeric index
        emitter.instruction(&format!("str x9, [x0, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // persist incremented LIFO iterator index
        emitter.instruction("ret");                                             // return void
    }
    emitter.label(fifo_label);
    if forward {
        emitter.instruction("add x9, x9, #1");                                  // moving forward in FIFO increases the index
    } else {
        emitter.instruction("cbz x9, __rt_spl_dll_prev_fifo_done");             // moving before zero leaves the iterator exhausted at zero
        emitter.instruction("sub x9, x9, #1");                                  // otherwise move one FIFO slot backward
    }
    emitter.instruction(&format!("str x9, [x0, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // persist updated FIFO iterator index
    emitter.label(done_label);
    if !forward {
        emitter.label("__rt_spl_dll_prev_fifo_done");
    }
    emitter.instruction("ret");                                                 // return void
    if forward {
        emit_iterator_delete_step_aarch64(emitter, delete_label);
    }
}

fn emit_iterator_delete_step_aarch64(emitter: &mut Emitter, delete_label: &str) {
    emitter.label(delete_label);
    emitter.instruction("sub sp, sp, #32");                                     // reserve delete-mode iterator frame
    emitter.instruction("stp x29, x30, [sp, #16]");                             // save frame pointer and return address
    emitter.instruction("add x29, sp, #16");                                    // establish delete-mode iterator frame
    emitter.instruction("str x0, [sp, #0]");                                    // preserve receiver across pop/shift and release
    emitter.instruction(&format!("tst x10, #{}", ITER_MODE_LIFO));              // choose which end delete-mode traversal removes from
    emitter.instruction("b.ne __rt_spl_dll_next_delete_lifo");                  // LIFO delete removes the tail element
    emitter.instruction("bl __rt_spl_dll_shift");                               // FIFO delete removes the head element and compacts storage
    emitter.instruction("bl __rt_decref_mixed");                                // release the removed storage-owned Mixed cell
    emitter.instruction("ldr x9, [sp, #0]");                                    // reload receiver after deletion
    emitter.instruction(&format!("str xzr, [x9, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // FIFO delete keeps iteration at the new head
    emitter.instruction("b __rt_spl_dll_next_delete_done");                     // finish delete-mode next()
    emitter.label("__rt_spl_dll_next_delete_lifo");
    emitter.instruction("bl __rt_spl_dll_pop");                                 // LIFO delete removes the current tail element
    emitter.instruction("bl __rt_decref_mixed");                                // release the removed storage-owned Mixed cell
    emitter.instruction("ldr x9, [sp, #0]");                                    // reload receiver after deletion
    emitter.instruction(&format!("ldr x10, [x9, #{}]", SPL_DLL_STORAGE_OFFSET)); // load storage to find the new tail
    emitter.instruction("ldr x10, [x10]");                                      // read storage length after deletion
    emitter.instruction("cbz x10, __rt_spl_dll_next_delete_empty");             // empty storage rewinds to index zero
    emitter.instruction("sub x10, x10, #1");                                    // new LIFO current index is the new tail
    emitter.instruction(&format!("str x10, [x9, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // persist new LIFO delete cursor
    emitter.instruction("b __rt_spl_dll_next_delete_done");                     // finish non-empty LIFO delete
    emitter.label("__rt_spl_dll_next_delete_empty");
    emitter.instruction(&format!("str xzr, [x9, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // reset exhausted delete-mode iterator to zero
    emitter.label("__rt_spl_dll_next_delete_done");
    emitter.instruction("ldp x29, x30, [sp, #16]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #32");                                     // release delete-mode iterator frame
    emitter.instruction("ret");                                                 // return void
}

fn emit_valid_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_valid");
    emitter.instruction(&format!("ldr x9, [x0, #{}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("ldr x9, [x9]");                                        // read storage length
    emitter.instruction(&format!("ldr x10, [x0, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // read current iterator index
    emitter.instruction("cmp x10, x9");                                         // valid when index is below length
    emitter.instruction("cset x0, lo");                                         // return boolean validity
    emitter.instruction("ret");                                                 // return boolean result
}

fn emit_current_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_current");
    emitter.instruction("sub sp, sp, #32");                                     // reserve frame for the incref call
    emitter.instruction("stp x29, x30, [sp, #16]");                             // save frame pointer and return address
    emitter.instruction("add x29, sp, #16");                                    // establish a frame for nested incref
    emitter.instruction(&format!("ldr x9, [x0, #{}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("ldr x10, [x9]");                                       // read storage length
    emitter.instruction(&format!("ldr x11, [x0, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // read iterator index
    emitter.instruction("cmp x11, x10");                                        // is the iterator index inside storage?
    emitter.instruction("b.hs __rt_spl_dll_current_null");                      // invalid current() returns null
    emitter.instruction("add x12, x9, #24");                                    // point at first storage element
    emitter.instruction("ldr x0, [x12, x11, lsl #3]");                          // load the current Mixed cell
    emitter.instruction("bl __rt_incref");                                      // retain current Mixed cell for the caller
    emitter.instruction("ldp x29, x30, [sp, #16]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #32");                                     // release frame
    emitter.instruction("ret");                                                 // return retained Mixed cell
    emitter.label("__rt_spl_dll_current_null");
    emitter.instruction("ldp x29, x30, [sp, #16]");                             // restore frame pointer before null tail-call
    emitter.instruction("add sp, sp, #32");                                     // release frame before null tail-call
    emit_tail_boxed_null_aarch64(emitter);
}

fn emit_key_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_key");
    emitter.instruction(&format!("ldr x9, [x0, #{}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("ldr x9, [x9]");                                        // read storage length
    emitter.instruction(&format!("ldr x1, [x0, #{}]", SPL_DLL_ITER_INDEX_OFFSET)); // read iterator index as integer key payload
    emitter.instruction("cmp x1, x9");                                          // is iterator index valid?
    emitter.instruction("b.hs __rt_spl_dll_key_null");                          // invalid key() returns null
    emitter.instruction(&format!("mov x0, #{}", INT_TAG));                      // runtime tag 0 = int key
    emitter.instruction("mov x2, xzr");                                         // integer keys do not use a high payload word
    emitter.instruction("b __rt_mixed_from_value");                             // box and return the integer key
    emitter.label("__rt_spl_dll_key_null");
    emit_tail_boxed_null_aarch64(emitter);
}

fn emit_offset_exists_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_offset_exists");
    emit_offset_index_prefix_aarch64(emitter, "__rt_spl_dll_offset_exists_false");
    emitter.instruction("mov x0, #1");                                          // return true for any in-range offset
    emitter.instruction("ldp x29, x30, [sp, #48]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #64");                                     // release offset helper frame
    emitter.instruction("ret");                                                 // return boolean true
    emitter.label("__rt_spl_dll_offset_exists_false");
    emitter.instruction("mov x0, #0");                                          // return false for invalid offsets
    emitter.instruction("ldp x29, x30, [sp, #48]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #64");                                     // release offset helper frame
    emitter.instruction("ret");                                                 // return boolean false
}

fn emit_offset_get_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_offset_get");
    emit_offset_index_prefix_aarch64(emitter, "__rt_spl_dll_offset_get_null");
    emitter.instruction("add x11, x9, #24");                                    // point at first storage element
    emitter.instruction("ldr x0, [x11, x10, lsl #3]");                          // load selected Mixed cell
    emitter.instruction("bl __rt_incref");                                      // retain selected Mixed cell for caller
    emitter.instruction("ldp x29, x30, [sp, #48]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #64");                                     // release offset helper frame
    emitter.instruction("ret");                                                 // return retained Mixed cell
    emitter.label("__rt_spl_dll_offset_get_null");
    emitter.instruction("ldp x29, x30, [sp, #48]");                             // restore frame pointer before null return
    emitter.instruction("add sp, sp, #64");                                     // release offset helper frame before null return
    emit_tail_boxed_null_aarch64(emitter);
}

fn emit_offset_index_prefix_aarch64(emitter: &mut Emitter, invalid_label: &str) {
    emitter.instruction("sub sp, sp, #64");                                     // reserve common offset helper frame
    emitter.instruction("stp x29, x30, [sp, #48]");                             // save frame pointer and return address
    emitter.instruction("add x29, sp, #48");                                    // establish a frame for Mixed unbox/release
    emitter.instruction("str x0, [sp, #0]");                                    // save receiver
    emitter.instruction("str x1, [sp, #8]");                                    // save boxed offset argument
    emitter.instruction("mov x0, x1");                                          // pass boxed offset to mixed_unbox
    emitter.instruction("bl __rt_mixed_unbox");                                 // unbox offset into tag and payload words
    emitter.instruction("str x0, [sp, #16]");                                   // save unboxed offset tag
    emitter.instruction("str x1, [sp, #24]");                                   // save unboxed integer payload candidate
    emitter.instruction("ldr x0, [sp, #8]");                                    // reload boxed offset argument
    emitter.instruction("bl __rt_decref_mixed");                                // release the owned boxed offset argument
    emitter.instruction("ldr x12, [sp, #16]");                                  // reload unboxed offset tag
    emitter.instruction(&format!("cmp x12, #{}", INT_TAG));                     // offset must be an integer for list addressing
    emitter.instruction(&format!("b.ne {}", invalid_label));                    // reject non-integer offsets
    emitter.instruction("ldr x10, [sp, #24]");                                  // reload integer offset payload
    emitter.instruction("cmp x10, #0");                                         // reject negative offsets
    emitter.instruction(&format!("b.lt {}", invalid_label));                    // negative offsets are invalid
    emitter.instruction("ldr x0, [sp, #0]");                                    // reload receiver
    emitter.instruction(&format!("ldr x9, [x0, #{}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("ldr x11, [x9]");                                       // read storage length
    emitter.instruction("cmp x10, x11");                                        // compare offset with length
    emitter.instruction(&format!("b.hs {}", invalid_label));                    // offsets past the end are invalid
}

fn emit_offset_set_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_offset_set");
    emitter.instruction("sub sp, sp, #80");                                     // reserve offset-set helper frame
    emitter.instruction("stp x29, x30, [sp, #64]");                             // save frame pointer and return address
    emitter.instruction("add x29, sp, #64");                                    // establish a frame for nested release/append calls
    emitter.instruction("str x0, [sp, #0]");                                    // save receiver
    emitter.instruction("str x1, [sp, #8]");                                    // save boxed offset argument
    emitter.instruction("str x2, [sp, #16]");                                   // save owned Mixed value argument
    emitter.instruction("mov x0, x1");                                          // pass boxed offset to mixed_unbox
    emitter.instruction("bl __rt_mixed_unbox");                                 // unbox offset into tag and payload words
    emitter.instruction("str x0, [sp, #24]");                                   // save offset tag
    emitter.instruction("str x1, [sp, #32]");                                   // save integer offset payload candidate
    emitter.instruction("ldr x0, [sp, #8]");                                    // reload boxed offset argument
    emitter.instruction("bl __rt_decref_mixed");                                // release boxed offset argument
    emitter.instruction("ldr x12, [sp, #24]");                                  // reload offset tag
    emitter.instruction(&format!("cmp x12, #{}", NULL_TAG));                    // null offset means append
    emitter.instruction("b.eq __rt_spl_dll_offset_set_append");                 // append when offset is null
    emitter.instruction(&format!("cmp x12, #{}", INT_TAG));                     // explicit offset must be integer
    emitter.instruction("b.ne __rt_spl_dll_offset_set_release_value");          // reject non-integer offsets and release value
    emitter.instruction("ldr x10, [sp, #32]");                                  // reload integer offset
    emitter.instruction("cmp x10, #0");                                         // reject negative offsets
    emitter.instruction("b.lt __rt_spl_dll_offset_set_release_value");          // release value for invalid negative offset
    emitter.instruction("ldr x9, [sp, #0]");                                    // reload receiver
    emitter.instruction(&format!("ldr x9, [x9, #{}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("ldr x11, [x9]");                                       // read storage length
    emitter.instruction("cmp x10, x11");                                        // compare explicit offset with current length
    emitter.instruction("b.eq __rt_spl_dll_offset_set_append");                 // offset at length appends
    emitter.instruction("b.hi __rt_spl_dll_offset_set_release_value");          // offsets beyond length are invalid in this runtime subset
    emitter.instruction("add x12, x9, #24");                                    // point at first storage element
    emitter.instruction("ldr x0, [x12, x10, lsl #3]");                          // load the old Mixed cell at this offset
    emitter.instruction("str x9, [sp, #40]");                                   // preserve storage across old-value release
    emitter.instruction("str x10, [sp, #48]");                                  // preserve offset across old-value release
    emitter.instruction("bl __rt_decref_mixed");                                // release old Mixed cell before overwriting
    emitter.instruction("ldr x9, [sp, #40]");                                   // reload storage after release
    emitter.instruction("ldr x10, [sp, #48]");                                  // reload offset after release
    emitter.instruction("add x12, x9, #24");                                    // point at first storage element
    emitter.instruction("ldr x13, [sp, #16]");                                  // reload owned replacement Mixed cell
    emitter.instruction("str x13, [x12, x10, lsl #3]");                         // store replacement Mixed cell
    emitter.instruction("b __rt_spl_dll_offset_set_done");                      // finish offsetSet
    emitter.label("__rt_spl_dll_offset_set_append");
    emitter.instruction("ldr x0, [sp, #0]");                                    // reload receiver for append
    emitter.instruction("ldr x1, [sp, #16]");                                   // reload owned Mixed value for append
    emitter.instruction("bl __rt_spl_dll_push");                                // append value using shared push helper
    emitter.instruction("b __rt_spl_dll_offset_set_done");                      // finish offsetSet after append
    emitter.label("__rt_spl_dll_offset_set_release_value");
    emitter.instruction("ldr x0, [sp, #16]");                                   // reload owned Mixed value rejected by invalid offset
    emitter.instruction("bl __rt_decref_mixed");                                // release rejected value to avoid leaking argument ownership
    emitter.label("__rt_spl_dll_offset_set_done");
    emitter.instruction("ldp x29, x30, [sp, #64]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #80");                                     // release offset-set helper frame
    emitter.instruction("ret");                                                 // return void
}

fn emit_offset_unset_aarch64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_offset_unset");
    emit_offset_index_prefix_aarch64(emitter, "__rt_spl_dll_offset_unset_done");
    emitter.instruction("add x12, x9, #24");                                    // point at first storage element
    emitter.instruction("ldr x0, [x12, x10, lsl #3]");                          // load removed Mixed cell
    emitter.instruction("str x9, [sp, #32]");                                   // preserve storage across removed-value release
    emitter.instruction("str x10, [sp, #40]");                                  // preserve removed index across release
    emitter.instruction("bl __rt_decref_mixed");                                // release removed Mixed cell
    emitter.instruction("ldr x9, [sp, #32]");                                   // reload storage after release
    emitter.instruction("ldr x10, [sp, #40]");                                  // reload removed index after release
    emitter.instruction("ldr x11, [x9]");                                       // reload old storage length
    emitter.instruction("add x12, x9, #24");                                    // point at first storage element
    emitter.instruction("add x13, x10, #1");                                    // start compaction after the removed slot
    emitter.label("__rt_spl_dll_offset_unset_shift_loop");
    emitter.instruction("cmp x13, x11");                                        // have all following elements shifted left?
    emitter.instruction("b.ge __rt_spl_dll_offset_unset_shrink");               // shrink once compaction is complete
    emitter.instruction("ldr x14, [x12, x13, lsl #3]");                         // load next Mixed pointer
    emitter.instruction("sub x15, x13, #1");                                    // compute destination index
    emitter.instruction("str x14, [x12, x15, lsl #3]");                         // shift Mixed pointer left by one slot
    emitter.instruction("add x13, x13, #1");                                    // advance compaction cursor
    emitter.instruction("b __rt_spl_dll_offset_unset_shift_loop");              // continue compaction
    emitter.label("__rt_spl_dll_offset_unset_shrink");
    emitter.instruction("sub x11, x11, #1");                                    // compute new length
    emitter.instruction("str x11, [x9]");                                       // persist shortened length
    emitter.instruction("str xzr, [x12, x11, lsl #3]");                         // clear stale tail slot
    emitter.label("__rt_spl_dll_offset_unset_done");
    emitter.instruction("ldp x29, x30, [sp, #48]");                             // restore frame pointer and return address
    emitter.instruction("add sp, sp, #64");                                     // release offset helper frame
    emitter.instruction("ret");                                                 // return void
}

fn emit_tail_boxed_null_aarch64(emitter: &mut Emitter) {
    emitter.instruction(&format!("mov x0, #{}", NULL_TAG));                     // runtime tag 8 = null
    emitter.instruction("mov x1, xzr");                                         // null payload low word is empty
    emitter.instruction("mov x2, xzr");                                         // null payload high word is empty
    emitter.instruction("b __rt_mixed_from_value");                             // tail-call boxed Mixed construction
}

fn emit_x86_64(emitter: &mut Emitter) {
    emitter.blank();
    emitter.comment("--- runtime: spl doubly linked list ---");
    emit_new_x86_64(emitter);
    emit_count_x86_64(emitter);
    emit_is_empty_x86_64(emitter);
    emit_push_x86_64(emitter);
    emit_pop_x86_64(emitter);
    emit_shift_x86_64(emitter);
    emit_unshift_x86_64(emitter);
    emit_insert_x86_64(emitter);
    emit_top_x86_64(emitter);
    emit_bottom_x86_64(emitter);
    emit_iterator_mode_x86_64(emitter);
    emit_rewind_x86_64(emitter);
    emit_next_prev_x86_64(emitter);
    emit_valid_x86_64(emitter);
    emit_current_x86_64(emitter);
    emit_key_x86_64(emitter);
    emit_offset_exists_x86_64(emitter);
    emit_offset_get_x86_64(emitter);
    emit_offset_set_x86_64(emitter);
    emit_offset_unset_x86_64(emitter);
}

fn emit_new_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_new");
    emitter.instruction("push rbp");                                            // preserve caller frame pointer for constructor spills
    emitter.instruction("mov rbp, rsp");                                        // establish constructor frame
    emitter.instruction("sub rsp, 16");                                         // reserve class-id and object-pointer spill slots
    emitter.instruction("mov QWORD PTR [rbp - 8], rdi");                        // save concrete SPL class id
    emitter.instruction(&format!("mov rax, {}", SPL_DLL_OBJECT_SIZE));          // request fixed SPL list object payload size
    emitter.instruction("call __rt_heap_alloc");                                // allocate the SPL list object payload
    emitter.instruction(&format!("mov r10, 0x{:x}", (X86_64_HEAP_MAGIC_HI32 << 32) | 4)); // materialize object heap kind with x86 marker
    emitter.instruction("mov QWORD PTR [rax - 8], r10");                        // stamp allocation as an object instance
    emitter.instruction("mov r10, QWORD PTR [rbp - 8]");                        // reload concrete SPL class id
    emitter.instruction("mov QWORD PTR [rax], r10");                            // store class id at object header
    emitter.instruction("mov QWORD PTR [rbp - 16], rax");                       // save object pointer while allocating storage
    emitter.instruction(&format!("mov rdi, {}", SPL_DLL_INITIAL_CAPACITY));     // initial internal storage capacity
    emitter.instruction("mov rsi, 8");                                          // each internal slot holds one Mixed pointer
    emitter.instruction("call __rt_array_new");                                 // allocate internal mixed-pointer storage
    emitter.instruction("mov r10, QWORD PTR [rax - 8]");                        // load internal array packed kind word
    emitter.instruction("or r10, 0x700");                                       // mark internal storage as an array of Mixed cells
    emitter.instruction("mov QWORD PTR [rax - 8], r10");                        // persist Mixed value_type tag on storage
    emitter.instruction("mov r11, QWORD PTR [rbp - 16]");                       // reload object pointer
    emitter.instruction(&format!("mov QWORD PTR [r11 + {}], rax", SPL_DLL_STORAGE_OFFSET)); // object.storage = internal Mixed array
    emitter.instruction(&format!("mov QWORD PTR [r11 + {}], 0", SPL_DLL_ITER_INDEX_OFFSET)); // iterator index starts at zero
    emitter.instruction(&format!("mov QWORD PTR [r11 + {}], 0", SPL_DLL_ITER_MODE_OFFSET)); // iterator mode starts FIFO/KEEP
    emitter.instruction("mov rax, r11");                                        // return initialized SPL object
    emitter.instruction("add rsp, 16");                                         // release constructor spills
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return object pointer
}

fn emit_count_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_count");
    emitter.instruction(&format!("mov r10, QWORD PTR [rdi + {}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("mov rax, QWORD PTR [r10]");                            // return internal storage length
    emitter.instruction("ret");                                                 // return count
}

fn emit_is_empty_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_is_empty");
    emitter.instruction(&format!("mov r10, QWORD PTR [rdi + {}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("cmp QWORD PTR [r10], 0");                              // compare storage length with zero
    emitter.instruction("sete al");                                             // set low byte when list is empty
    emitter.instruction("movzx rax, al");                                       // widen boolean result
    emitter.instruction("ret");                                                 // return boolean result
}

fn emit_push_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_push");
    emitter.instruction("push rbp");                                            // preserve caller frame pointer for append spill
    emitter.instruction("mov rbp, rsp");                                        // establish append frame
    emitter.instruction("sub rsp, 16");                                         // reserve receiver spill
    emitter.instruction("mov QWORD PTR [rbp - 8], rdi");                        // save receiver while appending
    emitter.instruction(&format!("mov rdi, QWORD PTR [rdi + {}]", SPL_DLL_STORAGE_OFFSET)); // pass internal storage as array_push receiver
    emitter.instruction("call __rt_array_push_int");                            // append owned Mixed pointer without retaining it again
    emitter.instruction("mov r10, QWORD PTR [rbp - 8]");                        // reload receiver after append
    emitter.instruction(&format!("mov QWORD PTR [r10 + {}], rax", SPL_DLL_STORAGE_OFFSET)); // store possibly-grown storage
    emitter.instruction("add rsp, 16");                                         // release append spill
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return void
}

fn emit_pop_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_pop");
    emitter.instruction(&format!("mov r9, QWORD PTR [rdi + {}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("mov r10, QWORD PTR [r9]");                             // read current storage length
    emitter.instruction("test r10, r10");                                       // is the list empty?
    emitter.instruction("jz __rt_spl_dll_pop_null");                            // empty list pops to null for now
    emitter.instruction("sub r10, 1");                                          // compute last occupied index
    emitter.instruction("mov QWORD PTR [r9], r10");                             // shrink storage length by one
    emitter.instruction("lea r11, [r9 + 24]");                                  // point at first storage element
    emitter.instruction("mov rax, QWORD PTR [r11 + r10 * 8]");                  // return removed Mixed cell, transferring ownership
    emitter.instruction("mov QWORD PTR [r11 + r10 * 8], 0");                    // clear stale slot beyond new length
    emitter.instruction("ret");                                                 // return removed Mixed cell
    emitter.label("__rt_spl_dll_pop_null");
    emit_tail_boxed_null_x86_64(emitter);
}

fn emit_shift_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_shift");
    emitter.instruction(&format!("mov r9, QWORD PTR [rdi + {}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("mov r10, QWORD PTR [r9]");                             // read current storage length
    emitter.instruction("test r10, r10");                                       // is the list empty?
    emitter.instruction("jz __rt_spl_dll_shift_null");                          // empty list shifts to null for now
    emitter.instruction("lea r11, [r9 + 24]");                                  // point at first storage element
    emitter.instruction("mov rax, QWORD PTR [r11]");                            // capture removed first Mixed cell
    emitter.instruction("mov r12, 1");                                          // start shifting from element index 1
    emitter.label("__rt_spl_dll_shift_loop");
    emitter.instruction("cmp r12, r10");                                        // have all live elements been shifted left?
    emitter.instruction("jge __rt_spl_dll_shift_done");                         // finish once cursor reaches old length
    emitter.instruction("mov r13, QWORD PTR [r11 + r12 * 8]");                  // load next Mixed pointer
    emitter.instruction("mov r14, r12");                                        // copy source index for destination calculation
    emitter.instruction("sub r14, 1");                                          // destination index is one slot earlier
    emitter.instruction("mov QWORD PTR [r11 + r14 * 8], r13");                  // move Mixed pointer down by one slot
    emitter.instruction("add r12, 1");                                          // advance shift cursor
    emitter.instruction("jmp __rt_spl_dll_shift_loop");                         // continue compacting storage
    emitter.label("__rt_spl_dll_shift_done");
    emitter.instruction("sub r10, 1");                                          // compute new storage length
    emitter.instruction("mov QWORD PTR [r9], r10");                             // persist shortened length
    emitter.instruction("mov QWORD PTR [r11 + r10 * 8], 0");                    // clear stale tail slot
    emitter.instruction("ret");                                                 // return removed Mixed cell
    emitter.label("__rt_spl_dll_shift_null");
    emit_tail_boxed_null_x86_64(emitter);
}

fn emit_unshift_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_unshift");
    emitter.instruction("mov rdx, rsi");                                        // move value to insert helper's value argument
    emitter.instruction("xor rsi, rsi");                                        // unshift inserts at index zero
    emitter.instruction("jmp __rt_spl_dll_insert");                             // tail-call shared insertion helper
}

fn emit_insert_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_insert");
    emitter.instruction("push rbp");                                            // preserve caller frame pointer for insertion state
    emitter.instruction("mov rbp, rsp");                                        // establish insertion frame
    emitter.instruction("sub rsp, 48");                                         // reserve receiver, index, value, storage, and length spills
    emitter.instruction("mov QWORD PTR [rbp - 8], rdi");                        // save receiver
    emitter.instruction("mov QWORD PTR [rbp - 16], rsi");                       // save requested insertion index
    emitter.instruction("mov QWORD PTR [rbp - 24], rdx");                       // save owned Mixed value to insert
    emitter.instruction("cmp rsi, 0");                                          // is requested index negative?
    emitter.instruction("jge __rt_spl_dll_insert_index_nonnegative");           // keep non-negative indexes
    emitter.instruction("mov QWORD PTR [rbp - 16], 0");                         // clamp negative indexes to the beginning
    emitter.label("__rt_spl_dll_insert_index_nonnegative");
    emitter.instruction(&format!("mov r9, QWORD PTR [rdi + {}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("mov QWORD PTR [rbp - 32], r9");                        // save current storage pointer
    emitter.instruction("mov r10, QWORD PTR [r9]");                             // read current storage length
    emitter.instruction("mov r11, QWORD PTR [r9 + 8]");                         // read current storage capacity
    emitter.instruction("mov r12, QWORD PTR [rbp - 16]");                       // reload requested insertion index
    emitter.instruction("cmp r12, r10");                                        // is index past the end?
    emitter.instruction("jle __rt_spl_dll_insert_index_in_range");              // keep indexes within append boundary
    emitter.instruction("mov QWORD PTR [rbp - 16], r10");                       // clamp indexes past end to append
    emitter.label("__rt_spl_dll_insert_index_in_range");
    emitter.instruction("cmp r10, r11");                                        // is storage full?
    emitter.instruction("jne __rt_spl_dll_insert_have_capacity");               // skip growth when capacity remains
    emitter.instruction("mov rdi, r9");                                         // pass current storage to array_grow
    emitter.instruction("call __rt_array_grow");                                // grow internal storage
    emitter.instruction("mov r9, QWORD PTR [rbp - 8]");                         // reload receiver after growth
    emitter.instruction(&format!("mov QWORD PTR [r9 + {}], rax", SPL_DLL_STORAGE_OFFSET)); // store possibly-grown storage
    emitter.instruction("mov QWORD PTR [rbp - 32], rax");                       // save grown storage
    emitter.label("__rt_spl_dll_insert_have_capacity");
    emitter.instruction("mov r9, QWORD PTR [rbp - 32]");                        // reload storage pointer
    emitter.instruction("mov r10, QWORD PTR [r9]");                             // reload current length
    emitter.instruction("mov r12, QWORD PTR [rbp - 16]");                       // reload clamped insertion index
    emitter.instruction("lea r11, [r9 + 24]");                                  // point at first storage element
    emitter.instruction("mov r13, r10");                                        // start right-shift cursor at old length
    emitter.label("__rt_spl_dll_insert_shift_loop");
    emitter.instruction("cmp r13, r12");                                        // has cursor reached insertion index?
    emitter.instruction("jle __rt_spl_dll_insert_store");                       // stop once insertion slot is free
    emitter.instruction("mov r14, r13");                                        // copy destination index for source calculation
    emitter.instruction("sub r14, 1");                                          // source index is one slot before cursor
    emitter.instruction("mov r15, QWORD PTR [r11 + r14 * 8]");                  // load Mixed pointer being shifted right
    emitter.instruction("mov QWORD PTR [r11 + r13 * 8], r15");                  // store Mixed pointer one slot to the right
    emitter.instruction("sub r13, 1");                                          // move shift cursor left
    emitter.instruction("jmp __rt_spl_dll_insert_shift_loop");                  // continue shifting until insert slot opens
    emitter.label("__rt_spl_dll_insert_store");
    emitter.instruction("mov r14, QWORD PTR [rbp - 24]");                       // reload owned Mixed value to insert
    emitter.instruction("mov QWORD PTR [r11 + r12 * 8], r14");                  // store owned Mixed value in insertion slot
    emitter.instruction("add r10, 1");                                          // increase storage length
    emitter.instruction("mov QWORD PTR [r9], r10");                             // persist new storage length
    emitter.instruction("add rsp, 48");                                         // release insertion state
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return void
}

fn emit_top_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_top");
    emit_peek_index_x86_64(emitter, "__rt_spl_dll_top_null", true);
}

fn emit_bottom_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_bottom");
    emit_peek_index_x86_64(emitter, "__rt_spl_dll_bottom_null", false);
}

fn emit_peek_index_x86_64(emitter: &mut Emitter, null_label: &str, last: bool) {
    emitter.instruction(&format!("mov r9, QWORD PTR [rdi + {}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("mov r10, QWORD PTR [r9]");                             // read storage length
    emitter.instruction("test r10, r10");                                       // is storage empty?
    emitter.instruction(&format!("jz {}", null_label));                         // empty storage returns null
    if last {
        emitter.instruction("sub r10, 1");                                      // choose last occupied index
    } else {
        emitter.instruction("xor r10, r10");                                    // choose first occupied index
    }
    emitter.instruction("lea r11, [r9 + 24]");                                  // point at first storage element
    emitter.instruction("mov rax, QWORD PTR [r11 + r10 * 8]");                  // load selected Mixed cell
    emitter.instruction("call __rt_incref");                                    // retain Mixed cell for caller while storage keeps owner
    emitter.instruction("ret");                                                 // return retained Mixed cell
    emitter.label(null_label);
    emit_tail_boxed_null_x86_64(emitter);
}

fn emit_iterator_mode_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_set_iterator_mode");
    emitter.instruction(&format!("mov QWORD PTR [rdi + {}], rsi", SPL_DLL_ITER_MODE_OFFSET)); // store iterator mode bits on receiver
    emitter.instruction("ret");                                                 // return void
    emitter.label_global("__rt_spl_dll_get_iterator_mode");
    emitter.instruction(&format!("mov rax, QWORD PTR [rdi + {}]", SPL_DLL_ITER_MODE_OFFSET)); // return iterator mode bits
    emitter.instruction("ret");                                                 // return integer mode
}

fn emit_rewind_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_rewind");
    emitter.instruction(&format!("mov r9, QWORD PTR [rdi + {}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("mov r10, QWORD PTR [r9]");                             // read storage length
    emitter.instruction("test r10, r10");                                       // is storage empty?
    emitter.instruction("jz __rt_spl_dll_rewind_empty");                        // empty storage rewinds to zero
    emitter.instruction(&format!("mov r11, QWORD PTR [rdi + {}]", SPL_DLL_ITER_MODE_OFFSET)); // load iterator mode bits
    emitter.instruction(&format!("test r11, {}", ITER_MODE_LIFO));              // is LIFO traversal requested?
    emitter.instruction("jz __rt_spl_dll_rewind_fifo");                         // FIFO traversal starts at zero
    emitter.instruction("sub r10, 1");                                          // LIFO traversal starts at last element
    emitter.instruction(&format!("mov QWORD PTR [rdi + {}], r10", SPL_DLL_ITER_INDEX_OFFSET)); // store starting LIFO index
    emitter.instruction("ret");                                                 // return void
    emitter.label("__rt_spl_dll_rewind_fifo");
    emitter.instruction(&format!("mov QWORD PTR [rdi + {}], 0", SPL_DLL_ITER_INDEX_OFFSET)); // store starting FIFO index
    emitter.instruction("ret");                                                 // return void
    emitter.label("__rt_spl_dll_rewind_empty");
    emitter.instruction(&format!("mov QWORD PTR [rdi + {}], 0", SPL_DLL_ITER_INDEX_OFFSET)); // reset empty iterator to zero
    emitter.instruction("ret");                                                 // return void
}

fn emit_next_prev_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_next");
    emit_iterator_step_x86_64(emitter, true);
    emitter.label_global("__rt_spl_dll_prev");
    emit_iterator_step_x86_64(emitter, false);
}

fn emit_iterator_step_x86_64(emitter: &mut Emitter, forward: bool) {
    let fifo_label = if forward {
        "__rt_spl_dll_next_fifo"
    } else {
        "__rt_spl_dll_prev_fifo"
    };
    let delete_label = if forward {
        "__rt_spl_dll_next_delete"
    } else {
        ""
    };
    let done_label = if forward {
        "__rt_spl_dll_next_done"
    } else {
        "__rt_spl_dll_prev_done"
    };
    emitter.instruction(&format!("mov r9, QWORD PTR [rdi + {}]", SPL_DLL_ITER_INDEX_OFFSET)); // load current iterator index
    emitter.instruction(&format!("mov r10, QWORD PTR [rdi + {}]", SPL_DLL_ITER_MODE_OFFSET)); // load iterator mode bits
    if forward {
        emitter.instruction(&format!("test r10, {}", ITER_MODE_DELETE));        // does next() need to delete the current element?
        emitter.instruction(&format!("jnz {}", delete_label));                  // delete-mode foreach advances by removing the current slot
    }
    emitter.instruction(&format!("test r10, {}", ITER_MODE_LIFO));              // is traversal currently LIFO?
    emitter.instruction(&format!("jz {}", fifo_label));                         // FIFO and LIFO move in opposite directions
    if forward {
        emitter.instruction("test r9, r9");                                     // is LIFO traversal at numeric index zero?
        emitter.instruction("jz __rt_spl_dll_next_lifo_exhaust");               // moving forward from zero exhausts the iterator
        emitter.instruction("sub r9, 1");                                       // otherwise move to previous numeric index
        emitter.instruction(&format!("mov QWORD PTR [rdi + {}], r9", SPL_DLL_ITER_INDEX_OFFSET)); // persist decremented LIFO iterator index
        emitter.instruction("ret");                                             // return void
        emitter.label("__rt_spl_dll_next_lifo_exhaust");
        emitter.instruction(&format!("mov r10, QWORD PTR [rdi + {}]", SPL_DLL_STORAGE_OFFSET)); // load storage to compute exhausted sentinel
        emitter.instruction("mov r10, QWORD PTR [r10]");                        // storage length is the invalid sentinel
        emitter.instruction(&format!("mov QWORD PTR [rdi + {}], r10", SPL_DLL_ITER_INDEX_OFFSET)); // store exhausted LIFO sentinel
        emitter.instruction("ret");                                             // return void
    } else {
        emitter.instruction("add r9, 1");                                       // moving prev in LIFO increases numeric index
        emitter.instruction(&format!("mov QWORD PTR [rdi + {}], r9", SPL_DLL_ITER_INDEX_OFFSET)); // persist incremented LIFO iterator index
        emitter.instruction("ret");                                             // return void
    }
    emitter.label(fifo_label);
    if forward {
        emitter.instruction("add r9, 1");                                       // moving forward in FIFO increases index
    } else {
        emitter.instruction("test r9, r9");                                     // is FIFO traversal already at zero?
        emitter.instruction("jz __rt_spl_dll_prev_fifo_done");                  // moving before zero leaves iterator at zero
        emitter.instruction("sub r9, 1");                                       // otherwise move one FIFO slot backward
    }
    emitter.instruction(&format!("mov QWORD PTR [rdi + {}], r9", SPL_DLL_ITER_INDEX_OFFSET)); // persist updated FIFO iterator index
    emitter.label(done_label);
    if !forward {
        emitter.label("__rt_spl_dll_prev_fifo_done");
    }
    emitter.instruction("ret");                                                 // return void
    if forward {
        emit_iterator_delete_step_x86_64(emitter, delete_label);
    }
}

fn emit_iterator_delete_step_x86_64(emitter: &mut Emitter, delete_label: &str) {
    emitter.label(delete_label);
    emitter.instruction("push rbp");                                            // preserve caller frame pointer for delete-mode next()
    emitter.instruction("mov rbp, rsp");                                        // establish delete-mode iterator frame
    emitter.instruction("sub rsp, 16");                                         // reserve receiver spill slot
    emitter.instruction("mov QWORD PTR [rbp - 8], rdi");                        // preserve receiver across pop/shift and release
    emitter.instruction(&format!("test r10, {}", ITER_MODE_LIFO));              // choose which end delete-mode traversal removes from
    emitter.instruction("jnz __rt_spl_dll_next_delete_lifo");                   // LIFO delete removes the tail element
    emitter.instruction("call __rt_spl_dll_shift");                             // FIFO delete removes the head element and compacts storage
    emitter.instruction("call __rt_decref_mixed");                              // release the removed storage-owned Mixed cell
    emitter.instruction("mov r9, QWORD PTR [rbp - 8]");                         // reload receiver after deletion
    emitter.instruction(&format!("mov QWORD PTR [r9 + {}], 0", SPL_DLL_ITER_INDEX_OFFSET)); // FIFO delete keeps iteration at the new head
    emitter.instruction("jmp __rt_spl_dll_next_delete_done");                   // finish delete-mode next()
    emitter.label("__rt_spl_dll_next_delete_lifo");
    emitter.instruction("call __rt_spl_dll_pop");                               // LIFO delete removes the current tail element
    emitter.instruction("call __rt_decref_mixed");                              // release the removed storage-owned Mixed cell
    emitter.instruction("mov r9, QWORD PTR [rbp - 8]");                         // reload receiver after deletion
    emitter.instruction(&format!("mov r10, QWORD PTR [r9 + {}]", SPL_DLL_STORAGE_OFFSET)); // load storage to find the new tail
    emitter.instruction("mov r10, QWORD PTR [r10]");                            // read storage length after deletion
    emitter.instruction("test r10, r10");                                       // did deletion empty the storage?
    emitter.instruction("jz __rt_spl_dll_next_delete_empty");                   // empty storage rewinds to index zero
    emitter.instruction("sub r10, 1");                                          // new LIFO current index is the new tail
    emitter.instruction(&format!("mov QWORD PTR [r9 + {}], r10", SPL_DLL_ITER_INDEX_OFFSET)); // persist new LIFO delete cursor
    emitter.instruction("jmp __rt_spl_dll_next_delete_done");                   // finish non-empty LIFO delete
    emitter.label("__rt_spl_dll_next_delete_empty");
    emitter.instruction(&format!("mov QWORD PTR [r9 + {}], 0", SPL_DLL_ITER_INDEX_OFFSET)); // reset exhausted delete-mode iterator to zero
    emitter.label("__rt_spl_dll_next_delete_done");
    emitter.instruction("add rsp, 16");                                         // release receiver spill slot
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return void
}

fn emit_valid_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_valid");
    emitter.instruction(&format!("mov r9, QWORD PTR [rdi + {}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("mov r9, QWORD PTR [r9]");                              // read storage length
    emitter.instruction(&format!("mov r10, QWORD PTR [rdi + {}]", SPL_DLL_ITER_INDEX_OFFSET)); // read current iterator index
    emitter.instruction("cmp r10, r9");                                         // valid when index is below length
    emitter.instruction("setb al");                                             // set boolean for unsigned index < length
    emitter.instruction("movzx rax, al");                                       // widen boolean result
    emitter.instruction("ret");                                                 // return boolean result
}

fn emit_current_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_current");
    emitter.instruction(&format!("mov r9, QWORD PTR [rdi + {}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("mov r10, QWORD PTR [r9]");                             // read storage length
    emitter.instruction(&format!("mov r11, QWORD PTR [rdi + {}]", SPL_DLL_ITER_INDEX_OFFSET)); // read iterator index
    emitter.instruction("cmp r11, r10");                                        // is iterator index inside storage?
    emitter.instruction("jae __rt_spl_dll_current_null");                       // invalid current returns null
    emitter.instruction("lea r12, [r9 + 24]");                                  // point at first storage element
    emitter.instruction("mov rax, QWORD PTR [r12 + r11 * 8]");                  // load current Mixed cell
    emitter.instruction("call __rt_incref");                                    // retain current Mixed cell for caller
    emitter.instruction("ret");                                                 // return retained Mixed cell
    emitter.label("__rt_spl_dll_current_null");
    emit_tail_boxed_null_x86_64(emitter);
}

fn emit_key_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_key");
    emitter.instruction(&format!("mov r9, QWORD PTR [rdi + {}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("mov r9, QWORD PTR [r9]");                              // read storage length
    emitter.instruction(&format!("mov rdi, QWORD PTR [rdi + {}]", SPL_DLL_ITER_INDEX_OFFSET)); // read iterator index as integer key payload
    emitter.instruction("cmp rdi, r9");                                         // is iterator index valid?
    emitter.instruction("jae __rt_spl_dll_key_null");                           // invalid key returns null
    emitter.instruction(&format!("mov rax, {}", INT_TAG));                      // runtime tag 0 = int key
    emitter.instruction("xor rsi, rsi");                                        // integer keys do not use a high payload word
    emitter.instruction("jmp __rt_mixed_from_value");                           // box and return the integer key
    emitter.label("__rt_spl_dll_key_null");
    emit_tail_boxed_null_x86_64(emitter);
}

fn emit_offset_exists_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_offset_exists");
    emit_offset_index_prefix_x86_64(emitter, "__rt_spl_dll_offset_exists_false");
    emitter.instruction("mov rax, 1");                                          // return true for any in-range offset
    emitter.instruction("add rsp, 48");                                         // release offset helper frame
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return boolean true
    emitter.label("__rt_spl_dll_offset_exists_false");
    emitter.instruction("xor rax, rax");                                        // return false for invalid offsets
    emitter.instruction("add rsp, 48");                                         // release offset helper frame
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return boolean false
}

fn emit_offset_get_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_offset_get");
    emit_offset_index_prefix_x86_64(emitter, "__rt_spl_dll_offset_get_null");
    emitter.instruction("lea r11, [r9 + 24]");                                  // point at first storage element
    emitter.instruction("mov rax, QWORD PTR [r11 + r10 * 8]");                  // load selected Mixed cell
    emitter.instruction("call __rt_incref");                                    // retain selected Mixed cell for caller
    emitter.instruction("add rsp, 48");                                         // release offset helper frame
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return retained Mixed cell
    emitter.label("__rt_spl_dll_offset_get_null");
    emitter.instruction("add rsp, 48");                                         // release offset helper frame before null return
    emitter.instruction("pop rbp");                                             // restore caller frame pointer before null return
    emit_tail_boxed_null_x86_64(emitter);
}

fn emit_offset_index_prefix_x86_64(emitter: &mut Emitter, invalid_label: &str) {
    emitter.instruction("push rbp");                                            // preserve caller frame pointer for offset helper
    emitter.instruction("mov rbp, rsp");                                        // establish offset helper frame
    emitter.instruction("sub rsp, 48");                                         // reserve receiver, offset, tag, and payload spills
    emitter.instruction("mov QWORD PTR [rbp - 8], rdi");                        // save receiver
    emitter.instruction("mov QWORD PTR [rbp - 16], rsi");                       // save boxed offset argument
    emitter.instruction("mov rax, rsi");                                        // pass boxed offset to mixed_unbox
    emitter.instruction("call __rt_mixed_unbox");                               // unbox offset into tag and payload words
    emitter.instruction("mov QWORD PTR [rbp - 24], rax");                       // save unboxed offset tag
    emitter.instruction("mov QWORD PTR [rbp - 32], rdi");                       // save unboxed integer payload candidate
    emitter.instruction("mov rax, QWORD PTR [rbp - 16]");                       // reload boxed offset argument
    emitter.instruction("call __rt_decref_mixed");                              // release owned boxed offset argument
    emitter.instruction("mov r12, QWORD PTR [rbp - 24]");                       // reload unboxed offset tag
    emitter.instruction(&format!("cmp r12, {}", INT_TAG));                      // offset must be an integer for list addressing
    emitter.instruction(&format!("jne {}", invalid_label));                     // reject non-integer offsets
    emitter.instruction("mov r10, QWORD PTR [rbp - 32]");                       // reload integer offset payload
    emitter.instruction("cmp r10, 0");                                          // reject negative offsets
    emitter.instruction(&format!("jl {}", invalid_label));                      // negative offsets are invalid
    emitter.instruction("mov rdi, QWORD PTR [rbp - 8]");                        // reload receiver
    emitter.instruction(&format!("mov r9, QWORD PTR [rdi + {}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("mov r11, QWORD PTR [r9]");                             // read storage length
    emitter.instruction("cmp r10, r11");                                        // compare offset with length
    emitter.instruction(&format!("jae {}", invalid_label));                     // offsets past end are invalid
}

fn emit_offset_set_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_offset_set");
    emitter.instruction("push rbp");                                            // preserve caller frame pointer for offsetSet
    emitter.instruction("mov rbp, rsp");                                        // establish offsetSet frame
    emitter.instruction("sub rsp, 64");                                         // reserve receiver, offset, value, tag, payload, and storage spills
    emitter.instruction("mov QWORD PTR [rbp - 8], rdi");                        // save receiver
    emitter.instruction("mov QWORD PTR [rbp - 16], rsi");                       // save boxed offset argument
    emitter.instruction("mov QWORD PTR [rbp - 24], rdx");                       // save owned Mixed value argument
    emitter.instruction("mov rax, rsi");                                        // pass boxed offset to mixed_unbox
    emitter.instruction("call __rt_mixed_unbox");                               // unbox offset into tag and payload words
    emitter.instruction("mov QWORD PTR [rbp - 32], rax");                       // save offset tag
    emitter.instruction("mov QWORD PTR [rbp - 40], rdi");                       // save integer offset payload candidate
    emitter.instruction("mov rax, QWORD PTR [rbp - 16]");                       // reload boxed offset argument
    emitter.instruction("call __rt_decref_mixed");                              // release boxed offset argument
    emitter.instruction("mov r12, QWORD PTR [rbp - 32]");                       // reload offset tag
    emitter.instruction(&format!("cmp r12, {}", NULL_TAG));                     // null offset means append
    emitter.instruction("je __rt_spl_dll_offset_set_append");                   // append when offset is null
    emitter.instruction(&format!("cmp r12, {}", INT_TAG));                      // explicit offset must be integer
    emitter.instruction("jne __rt_spl_dll_offset_set_release_value");           // reject non-integer offsets and release value
    emitter.instruction("mov r10, QWORD PTR [rbp - 40]");                       // reload integer offset
    emitter.instruction("cmp r10, 0");                                          // reject negative offsets
    emitter.instruction("jl __rt_spl_dll_offset_set_release_value");            // release value for invalid negative offset
    emitter.instruction("mov r9, QWORD PTR [rbp - 8]");                         // reload receiver
    emitter.instruction(&format!("mov r9, QWORD PTR [r9 + {}]", SPL_DLL_STORAGE_OFFSET)); // load internal storage
    emitter.instruction("mov r11, QWORD PTR [r9]");                             // read storage length
    emitter.instruction("cmp r10, r11");                                        // compare explicit offset with length
    emitter.instruction("je __rt_spl_dll_offset_set_append");                   // offset at length appends
    emitter.instruction("ja __rt_spl_dll_offset_set_release_value");            // offsets beyond length are invalid in this runtime subset
    emitter.instruction("lea r12, [r9 + 24]");                                  // point at first storage element
    emitter.instruction("mov rax, QWORD PTR [r12 + r10 * 8]");                  // load old Mixed cell at this offset
    emitter.instruction("mov QWORD PTR [rbp - 48], r9");                        // preserve storage across old-value release
    emitter.instruction("mov QWORD PTR [rbp - 56], r10");                       // preserve offset across old-value release
    emitter.instruction("call __rt_decref_mixed");                              // release old Mixed cell before overwriting
    emitter.instruction("mov r9, QWORD PTR [rbp - 48]");                        // reload storage after release
    emitter.instruction("mov r10, QWORD PTR [rbp - 56]");                       // reload offset after release
    emitter.instruction("lea r12, [r9 + 24]");                                  // point at first storage element
    emitter.instruction("mov r13, QWORD PTR [rbp - 24]");                       // reload owned replacement Mixed cell
    emitter.instruction("mov QWORD PTR [r12 + r10 * 8], r13");                  // store replacement Mixed cell
    emitter.instruction("jmp __rt_spl_dll_offset_set_done");                    // finish offsetSet
    emitter.label("__rt_spl_dll_offset_set_append");
    emitter.instruction("mov rdi, QWORD PTR [rbp - 8]");                        // reload receiver for append
    emitter.instruction("mov rsi, QWORD PTR [rbp - 24]");                       // reload owned Mixed value for append
    emitter.instruction("call __rt_spl_dll_push");                              // append value using shared push helper
    emitter.instruction("jmp __rt_spl_dll_offset_set_done");                    // finish offsetSet after append
    emitter.label("__rt_spl_dll_offset_set_release_value");
    emitter.instruction("mov rax, QWORD PTR [rbp - 24]");                       // reload owned Mixed value rejected by invalid offset
    emitter.instruction("call __rt_decref_mixed");                              // release rejected value to avoid leaking argument ownership
    emitter.label("__rt_spl_dll_offset_set_done");
    emitter.instruction("add rsp, 64");                                         // release offsetSet frame
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return void
}

fn emit_offset_unset_x86_64(emitter: &mut Emitter) {
    emitter.label_global("__rt_spl_dll_offset_unset");
    emit_offset_index_prefix_x86_64(emitter, "__rt_spl_dll_offset_unset_done");
    emitter.instruction("lea r12, [r9 + 24]");                                  // point at first storage element
    emitter.instruction("mov rax, QWORD PTR [r12 + r10 * 8]");                  // load removed Mixed cell
    emitter.instruction("mov QWORD PTR [rbp - 40], r9");                        // preserve storage across removed-value release
    emitter.instruction("mov QWORD PTR [rbp - 48], r10");                       // preserve removed index across release
    emitter.instruction("call __rt_decref_mixed");                              // release removed Mixed cell
    emitter.instruction("mov r9, QWORD PTR [rbp - 40]");                        // reload storage after release
    emitter.instruction("mov r10, QWORD PTR [rbp - 48]");                       // reload removed index after release
    emitter.instruction("mov r11, QWORD PTR [r9]");                             // reload old storage length
    emitter.instruction("lea r12, [r9 + 24]");                                  // point at first storage element
    emitter.instruction("lea r13, [r10 + 1]");                                  // start compaction after removed slot
    emitter.label("__rt_spl_dll_offset_unset_shift_loop");
    emitter.instruction("cmp r13, r11");                                        // have all following elements shifted left?
    emitter.instruction("jge __rt_spl_dll_offset_unset_shrink");                // shrink once compaction is complete
    emitter.instruction("mov r14, QWORD PTR [r12 + r13 * 8]");                  // load next Mixed pointer
    emitter.instruction("mov r15, r13");                                        // copy source index for destination calculation
    emitter.instruction("sub r15, 1");                                          // compute destination index
    emitter.instruction("mov QWORD PTR [r12 + r15 * 8], r14");                  // shift Mixed pointer left by one slot
    emitter.instruction("add r13, 1");                                          // advance compaction cursor
    emitter.instruction("jmp __rt_spl_dll_offset_unset_shift_loop");            // continue compaction
    emitter.label("__rt_spl_dll_offset_unset_shrink");
    emitter.instruction("sub r11, 1");                                          // compute new length
    emitter.instruction("mov QWORD PTR [r9], r11");                             // persist shortened length
    emitter.instruction("mov QWORD PTR [r12 + r11 * 8], 0");                    // clear stale tail slot
    emitter.label("__rt_spl_dll_offset_unset_done");
    emitter.instruction("add rsp, 48");                                         // release offset helper frame
    emitter.instruction("pop rbp");                                             // restore caller frame pointer
    emitter.instruction("ret");                                                 // return void
}

fn emit_tail_boxed_null_x86_64(emitter: &mut Emitter) {
    emitter.instruction(&format!("mov rax, {}", NULL_TAG));                     // runtime tag 8 = null
    emitter.instruction("xor rdi, rdi");                                        // null payload low word is empty
    emitter.instruction("xor rsi, rsi");                                        // null payload high word is empty
    emitter.instruction("jmp __rt_mixed_from_value");                           // tail-call boxed Mixed construction
}
