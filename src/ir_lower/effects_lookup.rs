//! Purpose:
//! Maps high-level calls encountered during AST-to-EIR lowering to conservative
//! EIR effect metadata.
//!
//! Called from:
//! - `crate::ir_lower::expr` when lowering builtins, user calls, externs, and
//!   runtime-shaped operations.
//!
//! Key details:
//! - This phase is deliberately conservative. Later EIR optimization phases can
//!   tighten effects once they consume richer call metadata.

use crate::ir::{Effects, Op};

/// Returns conservative effects for a builtin call.
pub(crate) fn builtin_effects(_name: &str) -> Effects {
    Op::BuiltinCall.default_effects()
}

/// Returns conservative effects for a user function call.
pub(crate) fn user_call_effects(_name: &str) -> Effects {
    Op::Call.default_effects()
}

/// Returns conservative effects for a runtime helper-shaped operation.
pub(crate) fn runtime_effects() -> Effects {
    Op::RuntimeCall.default_effects()
}
