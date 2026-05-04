use std::path::Path;

use crate::parser::ast::{Stmt, StmtKind};

pub(super) fn include_once_label(path: &Path) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("_include_once_{hash:016x}")
}

pub(super) fn split_include_once_declarations(stmts: Vec<Stmt>) -> (Vec<Stmt>, Vec<Stmt>) {
    let mut declarations = Vec::new();
    let mut executable = Vec::new();

    for stmt in stmts {
        match &stmt.kind {
            StmtKind::NamespaceDecl { .. } | StmtKind::UseDecl { .. } => {
                declarations.push(stmt.clone());
                executable.push(stmt);
            }
            StmtKind::NamespaceBlock { name, body } => {
                let (body_decls, body_exec) = split_include_once_declarations(body.clone());
                if !body_decls.is_empty() {
                    declarations.push(Stmt::new(
                        StmtKind::NamespaceBlock {
                            name: name.clone(),
                            body: body_decls,
                        },
                        stmt.span,
                    ));
                }
                if !body_exec.is_empty() {
                    executable.push(Stmt::new(
                        StmtKind::NamespaceBlock {
                            name: name.clone(),
                            body: body_exec,
                        },
                        stmt.span,
                    ));
                }
            }
            StmtKind::Synthetic(stmts) => {
                let (body_decls, body_exec) = split_include_once_declarations(stmts.clone());
                if !body_decls.is_empty() {
                    declarations.push(Stmt::new(StmtKind::Synthetic(body_decls), stmt.span));
                }
                if !body_exec.is_empty() {
                    executable.push(Stmt::new(StmtKind::Synthetic(body_exec), stmt.span));
                }
            }
            StmtKind::FunctionDecl { .. }
            | StmtKind::ClassDecl { .. }
            | StmtKind::EnumDecl { .. }
            | StmtKind::InterfaceDecl { .. }
            | StmtKind::TraitDecl { .. }
            | StmtKind::PackedClassDecl { .. }
            | StmtKind::ExternFunctionDecl { .. }
            | StmtKind::ExternClassDecl { .. }
            | StmtKind::ExternGlobalDecl { .. }
            | StmtKind::ConstDecl { .. } => declarations.push(stmt),
            _ => executable.push(stmt),
        }
    }

    (declarations, executable)
}
