use std::collections::HashSet;
use std::path::{Path, PathBuf};

mod contains;
mod engine;
mod exprs;
mod files;
mod include_once;
mod include_path;
mod state;
mod stmt_exprs;

use crate::errors::CompileError;
use crate::parser::ast::Program;

use contains::has_includes;
use engine::resolve_stmts;
use state::ResolveState;

/// Resolves all include/require statements by inlining the referenced files.
/// Runs between parsing and type checking.
pub fn resolve(program: Program, base_dir: &Path) -> Result<Program, CompileError> {
    if !has_includes(&program) {
        return Ok(program);
    }

    let mut declared_once: HashSet<PathBuf> = HashSet::new();
    let mut include_chain: Vec<PathBuf> = Vec::new();
    let mut state = ResolveState::default();
    resolve_stmts(
        program,
        base_dir,
        &mut declared_once,
        &mut include_chain,
        &mut state,
    )
}
