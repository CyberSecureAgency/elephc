//! ARM64 assembly emission for generator wrappers and resume functions.
//!
//! Each generator function `f` produces two emitted symbols:
//!
//! - `_fn_<f>` — the wrapper. Allocates the heap frame, copies parameters
//!   into their slots, zeroes locals/state, and returns the frame pointer.
//! - `_fn_<f>__resume` — the resume function. Dispatches on the frame's
//!   `state_idx` to either the body's entry point (state 0) or one of the
//!   per-yield resume labels, then runs the body until the next yield.

use super::model::*;
use crate::codegen::emit::Emitter;
use crate::codegen::runtime::generators::frame as gen_frame;

const OFF_PARAMS_BASE: usize = gen_frame::FIXED_HEADER_BYTES;

fn slot_offset(idx: usize) -> usize {
    OFF_PARAMS_BASE + idx * 8
}

pub(super) fn aligned_frame_size_with_slots(slot_count: usize) -> usize {
    gen_frame::aligned_frame_size(slot_count * 8)
}

// ---------------------------------------------------------------------------
// Expression and condition emission
// ---------------------------------------------------------------------------

fn emit_load_int_source(emitter: &mut Emitter, dest_reg: &str, src: &IntSource) {
    match src {
        IntSource::Literal(n) => {
            emitter.instruction(&format!("mov {}, #{}", dest_reg, n));      // load the literal int into the destination register
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
                emitter.instruction(&format!("mov {}, x0", dest_reg));      // move the function return value to the destination register
            }
        }
    }
}

/// Evaluate `args` into a temporary stack stash, pop them into
/// `x0..x{n-1}`, then `bl <fn_name>`. The return value remains in `x0`.
fn emit_int_function_call(emitter: &mut Emitter, fn_name: &str, args: &[IntSource]) {
    let n = args.len();
    let stash_bytes = if n == 0 { 0 } else { ((n * 8) + 15) & !15 };
    if stash_bytes > 0 {
        emitter.instruction(&format!("sub sp, sp, #{}", stash_bytes));      // reserve a 16-byte aligned slab for evaluated arguments
        for (i, arg) in args.iter().enumerate() {
            emit_load_int_source(emitter, "x9", arg);                       // x9 = computed argument value
            emitter.instruction(&format!("str x9, [sp, #{}]", i * 8));      // park argument i in its stash slot
        }
        for i in 0..n {
            emitter.instruction(&format!("ldr x{}, [sp, #{}]", i, i * 8));  // load argument i into its ABI register
        }
    }
    let label = crate::names::function_symbol(fn_name);
    emitter.instruction(&format!("bl {}", label));                          // branch with link into the user function
    if stash_bytes > 0 {
        emitter.instruction(&format!("add sp, sp, #{}", stash_bytes));      // release the argument stash
    }
}

fn emit_body_stmt(emitter: &mut Emitter, stmt: &BodyStmt) {
    match stmt {
        BodyStmt::AssignInt(idx, src) => {
            emit_load_int_source(emitter, "x1", src);
            emitter.instruction(&format!("str x1, [x19, #{}]", slot_offset(*idx))); // store the computed int into the local's slot
        }
        BodyStmt::AssignMixed(idx, src) => {
            // Mixed slots use the standard refcount-replace pattern: park
            // the previous Mixed pointer in x20, materialise the new
            // boxed pointer in x0, store, then decref the previous.
            let off = slot_offset(*idx);
            emit_replace_mixed_slot(emitter, off, |em| emit_box_mixed_source(em, src));
        }
        BodyStmt::PostIncrement(idx) => {
            emitter.instruction(&format!("ldr x1, [x19, #{}]", slot_offset(*idx))); // load the local's current value
            emitter.instruction("add x1, x1, #1");                          // increment the value by 1
            emitter.instruction(&format!("str x1, [x19, #{}]", slot_offset(*idx))); // write the incremented value back to the slot
        }
        BodyStmt::PostDecrement(idx) => {
            emitter.instruction(&format!("ldr x1, [x19, #{}]", slot_offset(*idx))); // load the local's current value
            emitter.instruction("sub x1, x1, #1");                          // decrement the value by 1
            emitter.instruction(&format!("str x1, [x19, #{}]", slot_offset(*idx))); // write the decremented value back to the slot
        }
    }
}

fn emit_branch_if_false(emitter: &mut Emitter, cond: &BoolExpr, false_label: &str) {
    emit_load_int_source(emitter, "x1", &cond.left);
    emit_load_int_source(emitter, "x2", &cond.right);
    emitter.instruction("cmp x1, x2");                                      // compare the two computed integers
    let inverse_cc = match cond.op {
        CmpOp::Lt => "ge",
        CmpOp::Le => "gt",
        CmpOp::Gt => "le",
        CmpOp::Ge => "lt",
        CmpOp::Eq => "ne",
        CmpOp::Ne => "eq",
    };
    emitter.instruction(&format!("b.{} {}", inverse_cc, false_label));      // branch if the condition is false
}

/// Materialize a Mixed-cell pointer for `src` in `x0`. Invoked at yield
/// time to box the payload before stashing it in the frame's
/// `last_key`/`last_value` slot.
fn emit_box_mixed_source(emitter: &mut Emitter, src: &MixedSource) {
    match src {
        MixedSource::Int(int_src) => {
            emit_load_int_source(emitter, "x1", int_src);                   // x1 = lo (the int payload)
            emitter.instruction("mov x2, xzr");                             // x2 = hi (unused for ints)
            emitter.instruction("mov x0, #0");                              // x0 = tag (0 = int)
            emitter.instruction("bl __rt_mixed_from_value");                // x0 = boxed Mixed pointer
        }
        MixedSource::Str { label, len } => {
            // `adr` only reaches ±1 MB; go through `adrp + add :lo12:`.
            crate::codegen::abi::emit_symbol_address(emitter, "x1", label); // x1 = pointer to interned string bytes
            emitter.instruction(&format!("mov x2, #{}", len));              // x2 = string length in bytes
            emitter.instruction("mov x0, #1");                              // x0 = tag (1 = string)
            emitter.instruction("bl __rt_mixed_from_value");                // x0 = boxed Mixed pointer
        }
        MixedSource::IntArrayLit(values) => {
            emit_box_int_array_literal(emitter, values);
        }
        MixedSource::MixedSlot(idx) => {
            // The slot already holds a boxed Mixed pointer; we share it
            // with the destination by incref'ing — the slot keeps its
            // own reference and the new owner gets one too.
            emitter.instruction(&format!("ldr x0, [x19, #{}]", slot_offset(*idx))); // load the boxed Mixed pointer from the slot
            emitter.instruction("bl __rt_incref");                          // retain a refcount for the new owner
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
    emitter.instruction(&format!("mov x0, #{}", payload_bytes));            // request bytes for the array header + slots
    emitter.instruction("bl __rt_heap_alloc");                              // x0 = pointer to array body
    emitter.instruction("mov x9, #1");                                      // heap kind 1 = indexed-int array
    emitter.instruction("str x9, [x0, #-8]");                               // stamp the heap header kind
    emitter.instruction(&format!("mov x9, #{}", n));                        // length value
    emitter.instruction("str x9, [x0, #0]");                                // store array length at +0
    emitter.instruction(&format!("mov x9, #{}", n));                        // capacity = length for a literal
    emitter.instruction("str x9, [x0, #8]");                                // store array capacity at +8
    emitter.instruction("str xzr, [x0, #16]");                              // zero the reserved third header word
    for (i, v) in values.iter().enumerate() {
        let off = 24 + i * 8;
        emitter.instruction(&format!("mov x9, #{}", v));                    // load element value
        emitter.instruction(&format!("str x9, [x0, #{}]", off));            // store element into the array body
    }
    // Box the array pointer as a Mixed cell with tag = 4 (indexed array).
    emitter.instruction("mov x1, x0");                                      // x1 = lo = array pointer
    emitter.instruction("mov x2, xzr");                                     // x2 = hi unused
    emitter.instruction("mov x0, #4");                                      // x0 = tag (4 = indexed array)
    emitter.instruction("bl __rt_mixed_from_value");                        // x0 = boxed Mixed pointer
}

// ---------------------------------------------------------------------------
// Wrapper
// ---------------------------------------------------------------------------

pub(super) fn emit_wrapper(
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
    emitter.instruction(&format!("sub sp, sp, #{}", prologue_bytes));       // reserve the wrapper's stack frame
    emitter.instruction(&format!("stp x29, x30, [sp, #{}]", param_save_bytes)); // save frame pointer and return address above the param stash
    emitter.instruction(&format!("add x29, sp, #{}", param_save_bytes));    // establish the wrapper's frame pointer

    for i in 0..int_param_count {
        emitter.instruction(&format!("str x{}, [sp, #{}]", i, i * 8));      // park parameter i in its stash slot
    }

    emitter.instruction(&format!("mov x0, #{}", frame_size));               // total frame size including parameter and local slots
    emitter.instruction("bl __rt_heap_alloc");                              // x0 = pointer to fresh GeneratorFrame

    emitter.instruction("mov x9, #4");                                      // heap kind 4 = object instance
    emitter.instruction("str x9, [x0, #-8]");                               // write kind into the uniform heap header

    emitter.instruction(&format!("mov x9, #{}", class_id));                 // load Generator's compile-time class id
    emitter.instruction(&format!("str x9, [x0, #{}]", gen_frame::OFF_CLASS_ID)); // class_id at OFF_CLASS_ID

    emitter.instruction(&format!("adr x9, {}", resume_label));              // load address of the resume function symbol
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
        emitter.instruction(&format!("ldr x9, [sp, #{}]", i * 8));          // reload saved parameter i from the stash
        emitter.instruction(&format!("str x9, [x0, #{}]", frame_off));      // store parameter i in its frame slot
    }

    for i in 0..int_local_count {
        let frame_off = slot_offset(int_param_count + i);
        emitter.instruction(&format!("str xzr, [x0, #{}]", frame_off));     // zero-initialize local i's frame slot
    }

    emitter.instruction(&format!("ldp x29, x30, [sp, #{}]", param_save_bytes)); // restore frame pointer and return address
    emitter.instruction(&format!("add sp, sp, #{}", prologue_bytes));       // release the wrapper's stack frame
    emitter.instruction("ret");                                             // return the frame pointer
}

// ---------------------------------------------------------------------------
// Resume function
// ---------------------------------------------------------------------------

struct LoopLabels {
    end: String,
    cont: String,
}

struct ResumeCtx<'a> {
    label: &'a str,
    term_label: String,
    end_label: String,
    next_label_id: u32,
    loop_stack: Vec<LoopLabels>,
}

impl<'a> ResumeCtx<'a> {
    fn fresh_label(&mut self, hint: &str) -> String {
        let id = self.next_label_id;
        self.next_label_id += 1;
        format!("{}_{}_{}", self.label, hint, id)
    }
}

pub(super) fn emit_resume(
    emitter: &mut Emitter,
    label: &str,
    nodes: &[ResumeNode],
    highest_state: u32,
    mixed_slot_indices: &[usize],
) {
    emitter.blank();
    emitter.comment(&format!("--- generator resume {} ---", label));
    emitter.label_global(label);

    emitter.instruction("sub sp, sp, #32");                                 // reserve frame: 16 bytes for fp/lr + 16 for x19/x20
    emitter.instruction("stp x29, x30, [sp, #16]");                         // save frame pointer and return address
    emitter.instruction("stp x19, x20, [sp, #0]");                          // save callee-saved x19/x20
    emitter.instruction("add x29, sp, #16");                                // establish the resume function's frame pointer
    emitter.instruction("mov x19, x0");                                     // x19 = generator frame pointer

    emitter.instruction(&format!("ldr w10, [x19, #{}]", gen_frame::OFF_STATE_IDX)); // load resume state index

    let term_label = format!("{}_terminated", label);
    let end_label = format!("{}_end", label);
    let entry_label = format!("{}_entry", label);

    emitter.instruction("cmp w10, #0");                                     // state 0 → entry
    emitter.instruction(&format!("b.eq {}", entry_label));
    for k in 1..=highest_state {
        emitter.instruction(&format!("cmp w10, #{}", k));
        emitter.instruction(&format!("b.eq {}_resume_{}", label, k));       // dispatch to yield K's resume label
    }
    emitter.instruction(&format!("b {}", term_label));                      // unknown state → terminate

    emitter.label(&entry_label);

    let mut ctx = ResumeCtx {
        label,
        term_label: term_label.clone(),
        end_label: end_label.clone(),
        next_label_id: 0,
        loop_stack: Vec::new(),
    };
    emit_nodes(emitter, nodes, &mut ctx);

    emitter.instruction(&format!("b {}", term_label));                      // body fell off the end → terminate

    emitter.label(&term_label);
    // Decref any Mixed-typed locals that still hold a cell; without this
    // each generator that yielded an array/string into a slot would leak
    // that cell when the generator terminates. The flag is set first so
    // a re-entry via `next()` returns immediately without re-running.
    emitter.instruction(&format!("ldr w10, [x19, #{}]", gen_frame::OFF_FLAGS));
    emitter.instruction(&format!("orr w10, w10, #{}", gen_frame::FLAG_TERMINATED));
    emitter.instruction(&format!("str w10, [x19, #{}]", gen_frame::OFF_FLAGS));
    for &idx in mixed_slot_indices {
        let off = slot_offset(idx);
        emitter.instruction(&format!("ldr x0, [x19, #{}]", off));            // load the Mixed pointer parked in the local slot
        emitter.instruction(&format!("str xzr, [x19, #{}]", off));            // clear the slot so a double-terminate cannot decref twice
        emitter.instruction("bl __rt_decref_mixed");                          // release our refcount on the cell (NULL is safe)
    }

    emitter.label(&end_label);
    emitter.instruction("ldp x19, x20, [sp, #0]");                          // restore callee-saved x19/x20
    emitter.instruction("ldp x29, x30, [sp, #16]");                         // restore frame pointer and return address
    emitter.instruction("add sp, sp, #32");                                 // release the resume function's frame
    emitter.instruction("ret");                                             // return to caller
}

fn emit_nodes(emitter: &mut Emitter, nodes: &[ResumeNode], ctx: &mut ResumeCtx) {
    for node in nodes {
        emit_node(emitter, node, ctx);
    }
}

fn emit_node(emitter: &mut Emitter, node: &ResumeNode, ctx: &mut ResumeCtx) {
    match node {
        ResumeNode::Stmt(s) => emit_body_stmt(emitter, s),
        ResumeNode::Yield(entry, state_idx) => emit_yield(emitter, entry, *state_idx, ctx),
        ResumeNode::YieldAssign { local_idx, local_ty, yield_entry, state_idx } => {
            emit_yield(emitter, yield_entry, *state_idx, ctx);
            match local_ty {
                SlotType::Int => emit_yield_assign_unbox_int(emitter, *local_idx, ctx),
                SlotType::Mixed => emit_yield_assign_store_mixed(emitter, *local_idx, ctx),
            }
        }
        ResumeNode::If { cond, then_body, else_body } => {
            let else_lbl = ctx.fresh_label("if_else");
            let end_lbl = ctx.fresh_label("if_end");
            emit_branch_if_false(emitter, cond, &else_lbl);
            emit_nodes(emitter, then_body, ctx);
            emitter.instruction(&format!("b {}", end_lbl));
            emitter.label(&else_lbl);
            emit_nodes(emitter, else_body, ctx);
            emitter.label(&end_lbl);
        }
        ResumeNode::For { init, cond, update, body } => {
            emit_nodes(emitter, init, ctx);
            let test_lbl = ctx.fresh_label("for_test");
            let cont_lbl = ctx.fresh_label("for_cont");
            let end_lbl = ctx.fresh_label("for_end");
            emitter.label(&test_lbl);
            emit_branch_if_false(emitter, cond, &end_lbl);
            ctx.loop_stack.push(LoopLabels { end: end_lbl.clone(), cont: cont_lbl.clone() });
            emit_nodes(emitter, body, ctx);
            ctx.loop_stack.pop();
            emitter.label(&cont_lbl);
            emit_nodes(emitter, update, ctx);
            emitter.instruction(&format!("b {}", test_lbl));
            emitter.label(&end_lbl);
        }
        ResumeNode::While { cond, body } => {
            let top_lbl = ctx.fresh_label("while_top");
            let end_lbl = ctx.fresh_label("while_end");
            emitter.label(&top_lbl);
            emit_branch_if_false(emitter, cond, &end_lbl);
            ctx.loop_stack.push(LoopLabels { end: end_lbl.clone(), cont: top_lbl.clone() });
            emit_nodes(emitter, body, ctx);
            ctx.loop_stack.pop();
            emitter.instruction(&format!("b {}", top_lbl));
            emitter.label(&end_lbl);
        }
        ResumeNode::DoWhile { cond, body } => {
            let top_lbl = ctx.fresh_label("do_top");
            let cond_lbl = ctx.fresh_label("do_cond");
            let end_lbl = ctx.fresh_label("do_end");
            emitter.label(&top_lbl);
            ctx.loop_stack.push(LoopLabels { end: end_lbl.clone(), cont: cond_lbl.clone() });
            emit_nodes(emitter, body, ctx);
            ctx.loop_stack.pop();
            emitter.label(&cond_lbl);
            emit_branch_if_false(emitter, cond, &end_lbl);
            emitter.instruction(&format!("b {}", top_lbl));
            emitter.label(&end_lbl);
        }
        ResumeNode::Break => emit_loop_jump(emitter, ctx, /* break_jump */ true),
        ResumeNode::Continue => emit_loop_jump(emitter, ctx, /* break_jump */ false),
        ResumeNode::Switch { subject, cases, default } => {
            emit_switch(emitter, subject, cases, default, ctx);
        }
        ResumeNode::YieldFromGenerator { source, state_idx } => {
            emit_yield_from_generator(emitter, source, *state_idx, ctx);
        }
        ResumeNode::Return(value) => {
            // Box the return value (if any) into the frame's return_value
            // slot using the standard refcount-replace pattern, then jump
            // to the terminator which sets the TERMINATED flag.
            if let Some(src) = value {
                emit_replace_mixed_slot(emitter, gen_frame::OFF_RETURN_VALUE, |em| {
                    emit_box_mixed_source(em, src);
                });
            }
            let term = ctx.term_label.clone();
            emitter.instruction(&format!("b {}", term));
        }
        ResumeNode::Block { stmts } => emit_nodes(emitter, stmts, ctx),
        ResumeNode::Bail => {
            let term = ctx.term_label.clone();
            emitter.instruction(&format!("b {}", term));
        }
    }
}

/// Branch out of the innermost loop (break) or back to its `cont` label
/// (continue). Outside a loop, fall through to the terminator.
fn emit_loop_jump(emitter: &mut Emitter, ctx: &mut ResumeCtx, break_jump: bool) {
    let target = ctx.loop_stack.last().map(|labels| {
        if break_jump { labels.end.clone() } else { labels.cont.clone() }
    });
    let lbl = target.unwrap_or_else(|| ctx.term_label.clone());
    emitter.instruction(&format!("b {}", lbl));
}

fn emit_switch(
    emitter: &mut Emitter,
    subject: &IntSource,
    cases: &[(Vec<i64>, Vec<ResumeNode>)],
    default: &[ResumeNode],
    ctx: &mut ResumeCtx,
) {
    let end_lbl = ctx.fresh_label("switch_end");
    let default_lbl = ctx.fresh_label("switch_default");
    let case_labels: Vec<String> = (0..cases.len())
        .map(|_| ctx.fresh_label("switch_case"))
        .collect();
    // Evaluate subject once into x1, then dispatch to the matching case.
    emit_load_int_source(emitter, "x1", subject);
    for (i, (values, _)) in cases.iter().enumerate() {
        for v in values {
            emitter.instruction(&format!("mov x2, #{}", v));                // load this case literal into the comparison register
            emitter.instruction("cmp x1, x2");                              // compare subject against the case literal
            emitter.instruction(&format!("b.eq {}", case_labels[i]));       // jump to the case body when matching
        }
    }
    emitter.instruction(&format!("b {}", default_lbl));                     // no case matched — fall through to the default branch
    // Cases fall through unless their body breaks.
    ctx.loop_stack.push(LoopLabels {
        end: end_lbl.clone(),
        cont: end_lbl.clone(),
    });
    for (i, (_, body)) in cases.iter().enumerate() {
        emitter.label(&case_labels[i]);
        emit_nodes(emitter, body, ctx);
    }
    emitter.label(&default_lbl);
    emit_nodes(emitter, default, ctx);
    ctx.loop_stack.pop();
    emitter.label(&end_lbl);
}

fn emit_yield(
    emitter: &mut Emitter,
    entry: &YieldEntry,
    state_idx: u32,
    ctx: &mut ResumeCtx,
) {
    // Each yield overwrites both Mixed-pointer slots in the frame; we
    // refcount-drop the previous occupant so we don't leak a cell per
    // yield. `__rt_decref_mixed` tolerates NULL (the wrapper's initial
    // state).
    emit_replace_mixed_slot(emitter, gen_frame::OFF_LAST_KEY, |em| {
        emit_compute_key(em, entry.key.as_ref());
    });
    emit_replace_mixed_slot(emitter, gen_frame::OFF_LAST_VALUE, |em| {
        emit_box_mixed_source(em, &entry.value);
    });

    emitter.instruction(&format!("mov w10, #{}", state_idx));               // bump state to this yield's resume index
    emitter.instruction(&format!("str w10, [x19, #{}]", gen_frame::OFF_STATE_IDX)); // store updated state_idx
    emitter.instruction(&format!("b {}", ctx.end_label));                   // jump to common epilogue
    emitter.label(&format!("{}_resume_{}", ctx.label, state_idx));          // resume label for this yield
}

/// Emit the runtime-delegation loop for `yield from <gen_func>(args)`.
///
/// The body is structured so that **one** state index covers every
/// iteration of the inner generator. The first call (rewind on the outer
/// generator) lands at the entry, allocates the inner generator, calls
/// its rewind, then enters the loop. Each call to outer.next() resumes
/// at the `resume_<state_idx>` label, advances the inner generator, and
/// loops back. When the inner generator is exhausted, the outer body
/// continues immediately after the `yield from`.
fn emit_yield_from_generator(
    emitter: &mut Emitter,
    source: &YieldFromSource,
    state_idx: u32,
    ctx: &mut ResumeCtx,
) {
    let loop_lbl = ctx.fresh_label("yield_from_loop");
    let end_lbl = ctx.fresh_label("yield_from_end");

    // -- materialise the inner generator pointer in x0 --
    match source {
        YieldFromSource::Call { fn_name, args } => {
            emit_int_function_call(emitter, fn_name, args);                 // x0 = inner generator pointer
        }
        YieldFromSource::IntSlot(idx) => {
            emitter.instruction(&format!("ldr x0, [x19, #{}]", slot_offset(*idx))); // x0 = raw Generator pointer (loaded from int-typed slot)
        }
        YieldFromSource::MixedSlot(idx) => {
            // The Mixed slot holds a boxed Mixed cell wrapping an Object
            // payload. Unbox to recover the raw Generator/Iterator
            // pointer; `__rt_mixed_unbox` returns the unboxed payload
            // in x1 (low word) and the type tag in x0.
            emitter.instruction(&format!("ldr x0, [x19, #{}]", slot_offset(*idx))); // x0 = boxed Mixed pointer
            emitter.instruction("bl __rt_mixed_unbox");                     // x1 = unboxed object pointer (low word)
            emitter.instruction("mov x0, x1");                              // x0 = inner generator pointer
        }
    }
    emitter.instruction(&format!("str x0, [x19, #{}]", gen_frame::OFF_DELEGATED_ITER)); // store the inner generator handle in the frame
    emitter.instruction("bl __rt_gen_rewind");                              // run inner up to its first yield (x0 already = inner)

    // -- delegation loop: entered both initially and on every resume --
    emitter.label(&loop_lbl);
    emitter.instruction(&format!("ldr x0, [x19, #{}]", gen_frame::OFF_DELEGATED_ITER)); // reload inner pointer for valid()
    emitter.instruction("bl __rt_gen_valid");                               // x0 = 1 if inner has more values, 0 otherwise
    emitter.instruction(&format!("cbz x0, {}", end_lbl));                   // inner exhausted — leave the delegation loop

    // -- forward inner.current() into outer.last_value with refcount --
    // `__rt_gen_current`/`__rt_gen_key` already incref the returned cell
    // before handing it back, so the pointer we store here owns its own
    // refcount alongside the inner generator's frame slot. The inner's
    // next yield-replace decrefs the inner's reference but leaves ours
    // alive.
    emit_replace_mixed_slot(emitter, gen_frame::OFF_LAST_VALUE, |em| {
        em.instruction(&format!("ldr x0, [x19, #{}]", gen_frame::OFF_DELEGATED_ITER)); // x0 = inner gen ptr
        em.instruction("bl __rt_gen_current");                              // x0 = owned boxed Mixed pointer for the inner's current value
    });
    emit_replace_mixed_slot(emitter, gen_frame::OFF_LAST_KEY, |em| {
        em.instruction(&format!("ldr x0, [x19, #{}]", gen_frame::OFF_DELEGATED_ITER)); // x0 = inner gen ptr
        em.instruction("bl __rt_gen_key");                                  // x0 = owned boxed Mixed pointer for the inner's current key
    });

    // -- bump state_idx and yield back to the outer caller --
    emitter.instruction(&format!("mov w10, #{}", state_idx));               // mark this yield-from's resume index
    emitter.instruction(&format!("str w10, [x19, #{}]", gen_frame::OFF_STATE_IDX)); // store updated state_idx
    emitter.instruction(&format!("b {}", ctx.end_label));                   // return to the outer caller via the resume epilogue
    emitter.label(&format!("{}_resume_{}", ctx.label, state_idx));          // resume label hit on each subsequent next()
    emitter.instruction(&format!("ldr x0, [x19, #{}]", gen_frame::OFF_DELEGATED_ITER)); // x0 = inner gen ptr
    emitter.instruction("bl __rt_gen_next");                                // advance inner one step
    emitter.instruction(&format!("b {}", loop_lbl));                        // loop back to re-check valid()

    emitter.label(&end_lbl);
    emitter.instruction(&format!("str xzr, [x19, #{}]", gen_frame::OFF_DELEGATED_ITER)); // clear delegated_iter so future yields don't re-enter the loop
    // Fall through to the caller's continuation of the outer body.
}

/// Helper for the boxed-pointer overwrite pattern: park the previous
/// pointer in x20, run `produce_new` (which leaves the new boxed Mixed
/// pointer in x0), store it into the slot at `slot_off`, then decref the
/// previous pointer.
fn emit_replace_mixed_slot(
    emitter: &mut Emitter,
    slot_off: usize,
    produce_new: impl FnOnce(&mut Emitter),
) {
    emitter.instruction(&format!("ldr x20, [x19, #{}]", slot_off));         // remember the previous boxed pointer
    produce_new(emitter);
    emitter.instruction(&format!("str x0, [x19, #{}]", slot_off));          // store the freshly boxed pointer
    emitter.instruction("mov x0, x20");                                     // x0 = previous boxed pointer (or NULL)
    emitter.instruction("bl __rt_decref_mixed");                            // release the previous boxed pointer (NULL is safe)
}

fn emit_compute_key(emitter: &mut Emitter, key: Option<&MixedSource>) {
    match key {
        Some(src) => emit_box_mixed_source(emitter, src),
        None => {
            // Auto-key: load + increment the counter, then box the read value.
            emitter.instruction(&format!("ldr x1, [x19, #{}]", gen_frame::OFF_AUTO_KEY_COUNTER)); // x1 = current auto-key
            emitter.instruction("add x9, x1, #1");                          // x9 = next auto-key
            emitter.instruction(&format!("str x9, [x19, #{}]", gen_frame::OFF_AUTO_KEY_COUNTER)); // store the incremented counter
            emitter.instruction("mov x2, xzr");                             // x2 = unused hi for an int
            emitter.instruction("mov x0, #0");                              // x0 = int tag
            emitter.instruction("bl __rt_mixed_from_value");                // x0 = boxed auto-key Mixed pointer
        }
    }
}

/// After the resume label of a `YieldAssign` whose LHS is an Int slot,
/// unbox the int payload that `Generator::send($v)` parked in the
/// frame's `sent_value` slot and store it into the local.
fn emit_yield_assign_unbox_int(emitter: &mut Emitter, local_idx: usize, ctx: &mut ResumeCtx) {
    let null_lbl = ctx.fresh_label("send_null");
    let done_lbl = ctx.fresh_label("send_done");
    emitter.instruction(&format!("ldr x0, [x19, #{}]", gen_frame::OFF_SENT_VALUE)); // load the boxed sent_value pointer
    emitter.instruction(&format!("cbz x0, {}", null_lbl));                          // jump to null path when no send was performed
    emitter.instruction("bl __rt_mixed_unbox");                                     // x1 = unboxed low payload
    emitter.instruction("mov x9, x1");                                              // save the unboxed int across the next branch
    emitter.instruction(&format!("b {}", done_lbl));
    emitter.label(&null_lbl);
    emitter.instruction("mov x9, xzr");                                             // no sent_value → assignment receives 0
    emitter.label(&done_lbl);
    emitter.instruction(&format!("str x9, [x19, #{}]", slot_offset(local_idx)));    // store the int into the assignment LHS local
    emitter.instruction(&format!("str xzr, [x19, #{}]", gen_frame::OFF_SENT_VALUE)); // clear sent_value for the next round
}

/// After the resume label of a `YieldAssign` whose LHS is a Mixed slot,
/// transfer the sent Mixed pointer into the slot. `Generator::send($v)`
/// stored the boxed pointer in `sent_value`; we transfer ownership of
/// that single refcount into the slot via a refcount-replace pattern
/// (decref the slot's previous occupant). When `next()` was used
/// instead of `send()`, the slot stays at whatever it previously held.
fn emit_yield_assign_store_mixed(
    emitter: &mut Emitter,
    local_idx: usize,
    ctx: &mut ResumeCtx,
) {
    let off = slot_offset(local_idx);
    let skip = ctx.fresh_label("send_mixed_skip");
    let done = ctx.fresh_label("send_mixed_done");
    emitter.instruction(&format!("ldr x9, [x19, #{}]", gen_frame::OFF_SENT_VALUE)); // x9 = boxed sent_value pointer
    emitter.instruction(&format!("cbz x9, {}", skip));                              // no send_value → keep slot unchanged
    emitter.instruction("mov x20, x9");                                             // park the sent pointer across the slot decref call
    emitter.instruction(&format!("str xzr, [x19, #{}]", gen_frame::OFF_SENT_VALUE)); // clear sent_value (slot now owns the refcount)
    emitter.instruction(&format!("ldr x0, [x19, #{}]", off));                       // x0 = previous slot occupant (or NULL)
    emitter.instruction(&format!("str x20, [x19, #{}]", off));                      // overwrite slot with the sent pointer
    emitter.instruction("bl __rt_decref_mixed");                                    // decref the previous occupant (NULL is safe)
    emitter.instruction(&format!("b {}", done));
    emitter.label(&skip);
    emitter.instruction(&format!("str xzr, [x19, #{}]", gen_frame::OFF_SENT_VALUE)); // clear sent_value defensively
    emitter.label(&done);
}
