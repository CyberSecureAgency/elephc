//! Purpose:
//! Linear-scan register allocator (Poletto-Sarkar) over EIR functions. Assigns
//! callee-saved registers to non-overlapping value live intervals, spilling to
//! the stack under register pressure, with separate integer and float pools.
//!
//! Called from:
//! - `crate::codegen_ir` per function, before instruction lowering.
//!
//! Key details:
//! - Only callee-saved registers are allocated. They survive calls (so values
//!   may live across calls without spilling) and are disjoint from the scratch
//!   and result registers the instruction emitters use.
//! - First cut: only single-word `NonHeap` scalars (`I64`, `F64`) are
//!   register-eligible, and never block parameters or branch arguments, which
//!   stay in stack slots so the existing block-parameter moves are unchanged.
//!   Generators and functions with exception handlers fall back to all-spilled.

use std::collections::{HashMap, HashSet};

use crate::codegen::platform::{Arch, Target};
use crate::ir::{Function, IrType, Op, Ownership, Terminator, ValueId};
use crate::ir_passes::allocation::Allocation;
use crate::ir_passes::intervals::{build_intervals, LiveInterval};
use crate::ir_passes::liveness::compute_liveness;

/// Computes a register allocation for `func` on `target`.
///
/// Runs liveness and interval analysis, then a linear scan that assigns
/// callee-saved registers to eligible intervals and spills the longest-lived
/// interval when a pool is exhausted. Generators and functions containing
/// exception handlers conservatively fall back to all-spilled.
pub fn allocate_registers(func: &Function, target: Target) -> Allocation {
    if func.flags.is_generator || has_exception_handlers(func) {
        return Allocation::all_spilled();
    }

    let liveness = compute_liveness(func);
    let intervals = build_intervals(func, &liveness);
    let ineligible = ineligible_values(func);

    let eligible: Vec<LiveInterval> = intervals
        .into_iter()
        .filter(|iv| is_eligible(func, iv, &ineligible))
        .collect();

    scan(&eligible, target)
}

/// Returns true when the function contains any exception handler. Such
/// functions are skipped because a thrown exception may clobber registers
/// before reaching a handler in this first cut.
fn has_exception_handlers(func: &Function) -> bool {
    func.instructions
        .iter()
        .any(|inst| inst.op == Op::TryPushHandler)
}

/// Collects values that must stay in stack slots regardless of their type:
/// block parameters and values passed as branch arguments. These feed the
/// slot-based block-parameter moves, which read them from their slots.
fn ineligible_values(func: &Function) -> HashSet<ValueId> {
    let mut ineligible = HashSet::new();
    for block in &func.blocks {
        for param in &block.params {
            ineligible.insert(*param);
        }
        if let Some(term) = &block.terminator {
            for arg in terminator_branch_args(term) {
                ineligible.insert(arg);
            }
        }
    }
    ineligible
}

/// Returns the values a terminator passes as block-parameter arguments. These
/// are distinct from condition/scrutinee/return uses, which are ordinary uses.
fn terminator_branch_args(term: &Terminator) -> Vec<ValueId> {
    match term {
        Terminator::Br { args, .. } => args.clone(),
        Terminator::CondBr {
            then_args,
            else_args,
            ..
        } => then_args.iter().chain(else_args).copied().collect(),
        Terminator::Switch {
            cases,
            default_args,
            ..
        } => cases
            .iter()
            .flat_map(|case| case.args.iter().copied())
            .chain(default_args.iter().copied())
            .collect(),
        Terminator::GeneratorSuspend { resume_args, .. } => resume_args.clone(),
        Terminator::Return { .. }
        | Terminator::Throw { .. }
        | Terminator::Fatal { .. }
        | Terminator::Unreachable => Vec::new(),
    }
}

/// Returns true when an interval's value can live in a register: a single-word
/// non-heap scalar that is not a block parameter or branch argument.
fn is_eligible(func: &Function, iv: &LiveInterval, ineligible: &HashSet<ValueId>) -> bool {
    if ineligible.contains(&iv.value) {
        return false;
    }
    if !matches!(iv.ir_type, IrType::I64 | IrType::F64) {
        return false;
    }
    func.value(iv.value)
        .map(|value| value.ownership == Ownership::NonHeap)
        .unwrap_or(false)
}

/// An interval currently holding a register during the scan.
struct ActiveInterval {
    /// Linear position where the interval ends.
    end: u32,
    /// Register the interval occupies.
    reg: &'static str,
    /// Value the interval describes.
    value: ValueId,
}

/// Runs the linear scan over already-eligible, start-sorted intervals.
///
/// Maintains per-pool free lists and an active set. On pool exhaustion it
/// spills the interval (active or current) whose end is furthest away, the
/// Poletto-Sarkar heuristic, so the shorter-lived value keeps the register.
fn scan(eligible: &[LiveInterval], target: Target) -> Allocation {
    let mut free_int: Vec<&'static str> = int_pool(target).iter().rev().copied().collect();
    let mut free_float: Vec<&'static str> = float_pool(target).iter().rev().copied().collect();
    let mut active: Vec<ActiveInterval> = Vec::new();
    let mut assignments: HashMap<ValueId, &'static str> = HashMap::new();
    let mut used: Vec<&'static str> = Vec::new();

    for iv in eligible {
        expire_old_intervals(&mut active, iv.start, &mut free_int, &mut free_float);

        let is_float = iv.ir_type == IrType::F64;
        let free = if is_float { &mut free_float } else { &mut free_int };

        if let Some(reg) = free.pop() {
            assignments.insert(iv.value, reg);
            used.push(reg);
            active.push(ActiveInterval {
                end: iv.end,
                reg,
                value: iv.value,
            });
        } else {
            spill_at_interval(iv, is_float, &mut active, &mut assignments);
        }
    }

    Allocation::from_assignments(assignments, used)
}

/// Frees registers held by intervals that end at or before `position`, so the
/// register can be reused by the interval starting there.
fn expire_old_intervals(
    active: &mut Vec<ActiveInterval>,
    position: u32,
    free_int: &mut Vec<&'static str>,
    free_float: &mut Vec<&'static str>,
) {
    active.retain(|a| {
        if a.end <= position {
            if is_float_reg(a.reg) {
                free_float.push(a.reg);
            } else {
                free_int.push(a.reg);
            }
            false
        } else {
            true
        }
    });
}

/// Resolves register pressure for `current` when its pool is empty: either
/// steals a register from the active interval that ends latest (spilling that
/// interval) or spills `current` itself, whichever lives longer stays.
fn spill_at_interval(
    current: &LiveInterval,
    is_float: bool,
    active: &mut Vec<ActiveInterval>,
    assignments: &mut HashMap<ValueId, &'static str>,
) {
    let furthest = active
        .iter()
        .enumerate()
        .filter(|(_, a)| is_float_reg(a.reg) == is_float)
        .max_by_key(|(_, a)| a.end);

    let Some((index, a)) = furthest else {
        // The pool is empty for this type; `current` stays spilled.
        return;
    };

    if a.end > current.end {
        let reg = a.reg;
        assignments.remove(&a.value);
        active.remove(index);
        assignments.insert(current.value, reg);
        active.push(ActiveInterval {
            end: current.end,
            reg,
            value: current.value,
        });
    }
    // Otherwise `current` has the furthest end and stays spilled.
}

/// Returns true when `reg` names a floating-point register.
fn is_float_reg(reg: &str) -> bool {
    reg.starts_with('d') || reg.starts_with("xmm")
}

/// Returns the integer callee-saved register pool for `target`, excluding the
/// frame pointer, scratch registers, and registers the emitters use inline.
fn int_pool(target: Target) -> &'static [&'static str] {
    match target.arch {
        Arch::AArch64 => &["x21", "x22", "x23", "x24", "x25", "x26", "x27", "x28"],
        // Only rbx is reliably preserved across the hand-written x86_64 runtime
        // routines and shared heap-marker codegen; r14/r15 are used there as
        // scratch without ABI-compliant save/restore, so they are not allocated.
        Arch::X86_64 => &["rbx"],
    }
}

/// Returns the float callee-saved register pool for `target`. SysV x86_64 has
/// no callee-saved XMM registers, so float values are never register-allocated
/// there and must stay spilled.
fn float_pool(target: Target) -> &'static [&'static str] {
    match target.arch {
        Arch::AArch64 => &["d8", "d9", "d10", "d11", "d12", "d13", "d14"],
        Arch::X86_64 => &[],
    }
}
