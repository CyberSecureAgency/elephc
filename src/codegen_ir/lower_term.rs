//! Purpose:
//! Lowers EIR block terminators into jumps, returns, exits, and future control-flow edges.
//!
//! Called from:
//! - `crate::codegen_ir::block_emit`.
//!
//! Key details:
//! - The current increment supports process-entry `return` and explicit unsupported
//!   diagnostics for branch/switch/throw paths that still need Phase 04 lowering.

use crate::ir::Terminator;

use super::context::FunctionContext;
use super::frame;
use super::{CodegenIrError, Result};

/// Lowers one EIR terminator.
pub(super) fn lower_terminator(ctx: &mut FunctionContext<'_>, term: &Terminator) -> Result<()> {
    match term {
        Terminator::Return { value: None } => {
            frame::emit_main_epilogue(ctx);
            Ok(())
        }
        Terminator::Return { value: Some(_) } => Err(CodegenIrError::unsupported(
            "return values on the EIR backend entry function",
        )),
        Terminator::Unreachable => Ok(()),
        Terminator::Br { .. } => Err(CodegenIrError::unsupported("br terminator")),
        Terminator::CondBr { .. } => Err(CodegenIrError::unsupported("cond_br terminator")),
        Terminator::Switch { .. } => Err(CodegenIrError::unsupported("switch terminator")),
        Terminator::Throw { .. } => Err(CodegenIrError::unsupported("throw terminator")),
        Terminator::Fatal { .. } => Err(CodegenIrError::unsupported("fatal terminator")),
        Terminator::GeneratorSuspend { .. } => {
            Err(CodegenIrError::unsupported("generator_suspend terminator"))
        }
    }
}
