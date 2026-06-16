//! Purpose:
//! Builds linear-program-order live intervals for EIR values from per-block
//! liveness. Intervals are the input to the linear-scan register allocator.
//!
//! Called from:
//! - `crate::ir_passes::regalloc` (register allocation core).
//!
//! Key details:
//! - Blocks are numbered in reverse postorder. Each block reserves a position
//!   for its parameters (block entry), one per instruction, and one for its
//!   terminator. Intervals are contiguous `[start, end]` ranges (classic
//!   Poletto-Sarkar): a value is conservatively considered live across any hole
//!   between its definition and its last use.

use std::collections::HashMap;

use crate::ir::{BlockId, Function, InstId, IrType, ValueDef, ValueId};
use crate::ir_passes::cfg::reverse_postorder;
use crate::ir_passes::liveness::{terminator_uses, LivenessInfo};

/// A value's contiguous live range in linear program order: live from `start`
/// (its definition point) through `end` (its last use or last live-out point).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveInterval {
    /// The value this interval describes.
    pub value: ValueId,
    /// The value's IR type, used by the allocator to pick a register pool.
    pub ir_type: IrType,
    /// Linear position of the value's definition.
    pub start: u32,
    /// Linear position of the value's last use or last live-out edge.
    pub end: u32,
}

/// Linear position numbering for a function: where each block's parameters,
/// instructions, and terminator sit on the single linear axis the allocator
/// scans.
struct LinearNumbering {
    /// Position assigned to a block's entry (where its parameters are defined).
    block_start: HashMap<BlockId, u32>,
    /// Position assigned to a block's terminator (its last live-out point).
    block_end: HashMap<BlockId, u32>,
    /// Position assigned to each instruction.
    inst_pos: HashMap<InstId, u32>,
    /// Blocks in reverse postorder, the order positions were assigned in.
    order: Vec<BlockId>,
}

/// Numbers blocks in reverse postorder. Each block reserves one position for
/// its entry/parameters, one per instruction, and one for its terminator, so
/// definitions precede their dominated uses on the linear axis.
fn number_positions(func: &Function) -> LinearNumbering {
    let order = reverse_postorder(func);
    let mut block_start = HashMap::new();
    let mut block_end = HashMap::new();
    let mut inst_pos = HashMap::new();
    let mut pos = 0u32;

    for &block_id in &order {
        let block = func.block(block_id).expect("ordered block exists");
        block_start.insert(block_id, pos);
        pos += 1;
        for inst_id in &block.instructions {
            inst_pos.insert(*inst_id, pos);
            pos += 1;
        }
        block_end.insert(block_id, pos);
        pos += 1;
    }

    LinearNumbering {
        block_start,
        block_end,
        inst_pos,
        order,
    }
}

/// Builds one contiguous live interval per value defined in a reachable block.
///
/// The start is the value's definition position. The end is the maximum of its
/// use positions (instruction operands and terminator uses) and the terminator
/// position of every block where the value is live-out, so values that stay
/// live across edges and loop back-edges span the intervening positions.
pub fn build_intervals(func: &Function, liveness: &LivenessInfo) -> Vec<LiveInterval> {
    let numbering = number_positions(func);

    let mut starts: HashMap<ValueId, u32> = HashMap::new();
    let mut ends: HashMap<ValueId, u32> = HashMap::new();

    // Seed each value with its definition position. Values defined in blocks
    // unreachable from entry have no position and are skipped entirely.
    for (index, value) in func.values.iter().enumerate() {
        let value_id = ValueId::from_raw(index as u32);
        let def_pos = match &value.def {
            ValueDef::BlockParam { block, .. } => numbering.block_start.get(block).copied(),
            ValueDef::Instruction { inst, .. } => numbering.inst_pos.get(inst).copied(),
        };
        if let Some(def_pos) = def_pos {
            starts.insert(value_id, def_pos);
            ends.insert(value_id, def_pos);
        }
    }

    let extend = |value: ValueId, position: u32, ends: &mut HashMap<ValueId, u32>| {
        if let Some(end) = ends.get_mut(&value) {
            if position > *end {
                *end = position;
            }
        }
    };

    for &block_id in &numbering.order {
        let block = func.block(block_id).expect("ordered block exists");

        for inst_id in &block.instructions {
            let position = numbering.inst_pos[inst_id];
            let inst = func.instruction(*inst_id).expect("valid instruction");
            for operand in &inst.operands {
                extend(*operand, position, &mut ends);
            }
        }

        let terminator_position = numbering.block_end[&block_id];
        if let Some(term) = &block.terminator {
            for value in terminator_uses(term) {
                extend(value, terminator_position, &mut ends);
            }
        }
        for value in liveness.live_out_of(block_id) {
            extend(*value, terminator_position, &mut ends);
        }
    }

    let mut intervals: Vec<LiveInterval> = starts
        .into_iter()
        .map(|(value, start)| LiveInterval {
            value,
            ir_type: func.value(value).expect("value exists").ir_type,
            start,
            end: ends[&value].max(start),
        })
        .collect();
    intervals.sort_by_key(|iv| (iv.start, iv.value.as_raw()));
    intervals
}
