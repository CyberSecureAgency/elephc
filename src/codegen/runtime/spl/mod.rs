//! Purpose:
//! Collects runtime helpers for SPL classes whose PHP surface is backed by custom storage.
//! The current module owns the Phase 4 doubly-linked-list family payload layout.
//!
//! Called from:
//! - `crate::codegen::runtime::emitters::emit_runtime()`.
//!
//! Key details:
//! - SPL payload offsets are shared with object cleanup and allocation helpers.

mod doubly_linked_list;
mod fixed_array;

pub(crate) const SPL_DLL_STORAGE_OFFSET: usize = 8;
pub(crate) const SPL_DLL_ITER_INDEX_OFFSET: usize = 16;
pub(crate) const SPL_DLL_ITER_MODE_OFFSET: usize = 24;
pub(crate) const SPL_FIXED_STORAGE_OFFSET: usize = 8;

pub(crate) use doubly_linked_list::emit_doubly_linked_list_runtime;
pub(crate) use fixed_array::emit_fixed_array_runtime;
