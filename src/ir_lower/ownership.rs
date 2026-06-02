//! Purpose:
//! Provides small helpers for explicit EIR ownership operations during
//! AST-to-EIR lowering.
//!
//! Called from:
//! - `crate::ir_lower::stmt` and `crate::ir_lower::expr` when values cross
//!   assignment, call, and cleanup boundaries.
//!
//! Key details:
//! - Ownership is represented by explicit EIR opcodes even though the legacy
//!   backend is still the production path.

#![allow(dead_code)]

use crate::ir::{Op, Ownership};
use crate::ir_lower::context::{LoweredValue, LoweringContext};
use crate::span::Span;

/// Emits an acquire operation when the value can carry runtime lifetime state.
pub(crate) fn acquire_if_refcounted(ctx: &mut LoweringContext<'_, '_>, value: LoweredValue, span: Option<Span>) {
    if value.ir_type.is_refcounted_storage() {
        ctx.emit_value(
            Op::Acquire,
            vec![value.value],
            None,
            ctx.builder.value_php_type(value.value),
            Op::Acquire.default_effects(),
            span,
        );
    }
}

/// Emits a release operation when the value can carry runtime lifetime state.
pub(crate) fn release_if_owned(ctx: &mut LoweringContext<'_, '_>, value: LoweredValue, span: Option<Span>) {
    if value.ir_type.is_refcounted_storage()
        && !matches!(ctx.builder.value_php_type(value.value), crate::types::PhpType::Void)
    {
        ctx.emit_void(
            Op::Release,
            vec![value.value],
            None,
            Op::Release.default_effects(),
            span,
        );
    }
}

/// Returns whether an ownership state means the value is potentially released by this path.
pub(crate) fn may_require_release(ownership: Ownership) -> bool {
    matches!(ownership, Ownership::Owned | Ownership::MaybeOwned)
}
