//! Purpose:
//! Generates stable labels used to track `include_once` and `require_once` execution.
//! Converts file paths into assembly-safe identifiers for resolver/runtime coordination.
//!
//! Called from:
//! - `crate::resolver::engine_includes::resolve_include_stmt()`.
//!
//! Key details:
//! - Labels must be deterministic for a canonical path so repeated includes share one guard.

use std::path::Path;

pub(super) fn include_once_label(path: &Path) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("_include_once_{hash:016x}")
}
