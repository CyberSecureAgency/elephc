//! Purpose:
//! Expression lowering, Mixed-cell boxing, and the refcount-replace pattern
//! for boxed pointer slots in the generator frame. Shared by statement
//! lowering and yield emission.
//!
//! Called from:
//!  - `super::stmts` (body statement and switch subject emission).
//!  - `super::yields` (yield value/key boxing, send resume helpers).
//!  - `super::dispatcher` (return-value boxing on terminal `return`).
//!
//! Key details:
//!  - All boxed Mixed-cell allocations go through `__rt_mixed_from_value` so
//!    the runtime tag/payload contract stays consistent with the rest of the
//!    type system.
//!  - `emit_replace_mixed_slot` is the canonical refcount-replace pattern:
//!    park the previous pointer in `x20`, produce the new pointer in `x0`,
//!    overwrite the slot, then decref the previous pointer (NULL is safe).

use super::slot_offset;
use super::super::model::*;
use crate::codegen::emit::Emitter;
use crate::codegen::runtime::generators::frame as gen_frame;

pub(super) fn emit_load_int_source(emitter: &mut Emitter, dest_reg: &str, src: &IntSource) {
    match src {
        IntSource::Literal(n) => {
            emitter.instruction(&format!("mov {}, #{}", dest_reg, n));          // load the literal int into the destination register
        }
        IntSource::Slot(idx) => {
            emitter.instruction(&format!("ldr {}, [x19, #{}]", dest_reg, slot_offset(*idx))); // load the int slot from the generator frame
        }
        IntSource::BinaryOp(left, op, right) => {
            emit_load_int_source(emitter, dest_reg, left);
            emit_load_int_source(emitter, "x12", right);
            let mnem = match op {
                IntBinOp::Add => "add",
                IntBinOp::Sub => "sub",
                IntBinOp::Mul => "mul",
                IntBinOp::Div => "sdiv",
            };
            emitter.instruction(&format!("{} {}, {}, x12", mnem, dest_reg, dest_reg)); // combine left and right with the chosen op
        }
        IntSource::Call { fn_name, args } => {
            emit_int_function_call(emitter, fn_name, args);
            if dest_reg != "x0" {
                emitter.instruction(&format!("mov {}, x0", dest_reg));          // move the function return value to the destination register
            }
        }
    }
}

/// Evaluate `args` into a temporary stack stash, pop them into
/// `x0..x{n-1}`, then `bl <fn_name>`. The return value remains in `x0`.
pub(super) fn emit_int_function_call(emitter: &mut Emitter, fn_name: &str, args: &[IntSource]) {
    let n = args.len();
    let stash_bytes = if n == 0 { 0 } else { ((n * 8) + 15) & !15 };
    if stash_bytes > 0 {
        emitter.instruction(&format!("sub sp, sp, #{}", stash_bytes));          // reserve a 16-byte aligned slab for evaluated arguments
        for (i, arg) in args.iter().enumerate() {
            emit_load_int_source(emitter, "x9", arg);                       // x9 = computed argument value
            emitter.instruction(&format!("str x9, [sp, #{}]", i * 8));          // park argument i in its stash slot
        }
        for i in 0..n {
            emitter.instruction(&format!("ldr x{}, [sp, #{}]", i, i * 8));      // load argument i into its ABI register
        }
    }
    let label = crate::names::function_symbol(fn_name);
    emitter.instruction(&format!("bl {}", label));                              // branch with link into the user function
    if stash_bytes > 0 {
        emitter.instruction(&format!("add sp, sp, #{}", stash_bytes));          // release the argument stash
    }
}

/// Materialize a Mixed-cell pointer for `src` in `x0`. Invoked at yield
/// time to box the payload before stashing it in the frame's
/// `last_key`/`last_value` slot.
pub(super) fn emit_box_mixed_source(emitter: &mut Emitter, src: &MixedSource) {
    match src {
        MixedSource::Int(int_src) => {
            emit_load_int_source(emitter, "x1", int_src);                   // x1 = lo (the int payload)
            emitter.instruction("mov x2, xzr");                                 // x2 = hi (unused for ints)
            emitter.instruction("mov x0, #0");                                  // x0 = tag (0 = int)
            emitter.instruction("bl __rt_mixed_from_value");                    // x0 = boxed Mixed pointer
        }
        MixedSource::Str { label, len } => {
            // `adr` only reaches ±1 MB; go through `adrp + add :lo12:`.
            crate::codegen::abi::emit_symbol_address(emitter, "x1", label); // x1 = pointer to interned string bytes
            emitter.instruction(&format!("mov x2, #{}", len));                  // x2 = string length in bytes
            emitter.instruction("mov x0, #1");                                  // x0 = tag (1 = string)
            emitter.instruction("bl __rt_mixed_from_value");                    // x0 = boxed Mixed pointer
        }
        MixedSource::IntArrayLit(values) => {
            emit_box_int_array_literal(emitter, values);
        }
        MixedSource::MixedSlot(idx) => {
            // The slot already holds a boxed Mixed pointer; we share it
            // with the destination by incref'ing — the slot keeps its
            // own reference and the new owner gets one too.
            emitter.instruction(&format!("ldr x0, [x19, #{}]", slot_offset(*idx))); // load the boxed Mixed pointer from the slot
            emitter.instruction("bl __rt_incref");                              // retain a refcount for the new owner
        }
    }
}

/// Allocate an indexed array of int values on the heap, populate it, and
/// box the resulting pointer as a Mixed cell with the array tag (4).
/// Layout matches `__rt_array_new`: 24-byte header (length, capacity,
/// reserved) followed by 8-byte slots.
fn emit_box_int_array_literal(emitter: &mut Emitter, values: &[i64]) {
    let n = values.len();
    let payload_bytes = 24 + n * 8;
    emitter.instruction(&format!("mov x0, #{}", payload_bytes));                // request bytes for the array header + slots
    emitter.instruction("bl __rt_heap_alloc");                                  // x0 = pointer to array body
    emitter.instruction("mov x9, #1");                                          // heap kind 1 = indexed-int array
    emitter.instruction("str x9, [x0, #-8]");                                   // stamp the heap header kind
    emitter.instruction(&format!("mov x9, #{}", n));                            // length value
    emitter.instruction("str x9, [x0, #0]");                                    // store array length at +0
    emitter.instruction(&format!("mov x9, #{}", n));                            // capacity = length for a literal
    emitter.instruction("str x9, [x0, #8]");                                    // store array capacity at +8
    emitter.instruction("str xzr, [x0, #16]");                                  // zero the reserved third header word
    for (i, v) in values.iter().enumerate() {
        let off = 24 + i * 8;
        emitter.instruction(&format!("mov x9, #{}", v));                        // load element value
        emitter.instruction(&format!("str x9, [x0, #{}]", off));                // store element into the array body
    }
    // Box the array pointer as a Mixed cell with tag = 4 (indexed array).
    emitter.instruction("mov x1, x0");                                          // x1 = lo = array pointer
    emitter.instruction("mov x2, xzr");                                         // x2 = hi unused
    emitter.instruction("mov x0, #4");                                          // x0 = tag (4 = indexed array)
    emitter.instruction("bl __rt_mixed_from_value");                            // x0 = boxed Mixed pointer
}

pub(super) fn emit_compute_key(emitter: &mut Emitter, key: Option<&MixedSource>) {
    match key {
        Some(src) => emit_box_mixed_source(emitter, src),
        None => {
            // Auto-key: load + increment the counter, then box the read value.
            emitter.instruction(&format!("ldr x1, [x19, #{}]", gen_frame::OFF_AUTO_KEY_COUNTER)); // x1 = current auto-key
            emitter.instruction("add x9, x1, #1");                              // x9 = next auto-key
            emitter.instruction(&format!("str x9, [x19, #{}]", gen_frame::OFF_AUTO_KEY_COUNTER)); // store the incremented counter
            emitter.instruction("mov x2, xzr");                                 // x2 = unused hi for an int
            emitter.instruction("mov x0, #0");                                  // x0 = int tag
            emitter.instruction("bl __rt_mixed_from_value");                    // x0 = boxed auto-key Mixed pointer
        }
    }
}

/// Helper for the boxed-pointer overwrite pattern: park the previous
/// pointer in x20, run `produce_new` (which leaves the new boxed Mixed
/// pointer in x0), store it into the slot at `slot_off`, then decref the
/// previous pointer.
pub(super) fn emit_replace_mixed_slot(
    emitter: &mut Emitter,
    slot_off: usize,
    produce_new: impl FnOnce(&mut Emitter),
) {
    emitter.instruction(&format!("ldr x20, [x19, #{}]", slot_off));             // remember the previous boxed pointer
    produce_new(emitter);
    emitter.instruction(&format!("str x0, [x19, #{}]", slot_off));              // store the freshly boxed pointer
    emitter.instruction("mov x0, x20");                                         // x0 = previous boxed pointer (or NULL)
    emitter.instruction("bl __rt_decref_mixed");                                // release the previous boxed pointer (NULL is safe)
}

/// Reused by `yields::emit_yield_assign_unbox_int` and friends — shared
/// branch that emits a comparison branch when a condition holds. Kept in
/// `values.rs` because it shares register conventions with the boxing
/// helpers.
pub(super) fn emit_branch_if_false(emitter: &mut Emitter, cond: &BoolExpr, false_label: &str) {
    emit_load_int_source(emitter, "x1", &cond.left);
    emit_load_int_source(emitter, "x2", &cond.right);
    emitter.instruction("cmp x1, x2");                                          // compare the two computed integers
    let inverse_cc = match cond.op {
        CmpOp::Lt => "ge",
        CmpOp::Le => "gt",
        CmpOp::Gt => "le",
        CmpOp::Ge => "lt",
        CmpOp::Eq => "ne",
        CmpOp::Ne => "eq",
    };
    emitter.instruction(&format!("b.{} {}", inverse_cc, false_label));          // branch if the condition is false
}
