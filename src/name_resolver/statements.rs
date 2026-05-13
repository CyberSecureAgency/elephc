//! Purpose:
//! Groups statement-list, context, and regular-statement name-resolution helpers.
//! Re-exports the entry points used by declarations and expression-contained statement bodies.
//!
//! Called from:
//! - `crate::name_resolver::resolve()` and nested resolvers.
//!
//! Key details:
//! - Statement resolution carries namespace/import context through lexical statement lists.

mod context;
mod list;
mod rewrite;

pub(super) use context::resolve_params;
pub(super) use list::resolve_stmt_list;
