use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::errors::CompileError;
use crate::parser::ast::{Stmt, StmtKind};

use super::engine::resolve_isolated;
use super::exprs::{resolve_expr, resolve_method_exprs, resolve_params, resolve_properties};
use super::state::ResolveState;

pub(super) fn resolve_stmt_exprs(
    stmt: Stmt,
    base_dir: &Path,
    declared_once: &mut HashSet<PathBuf>,
    include_chain: &mut Vec<PathBuf>,
    state: &ResolveState,
) -> Result<Stmt, CompileError> {
    let span = stmt.span;
    let kind = match stmt.kind {
        StmtKind::Synthetic(stmts) => StmtKind::Synthetic(resolve_isolated(
            stmts,
            base_dir,
            declared_once,
            include_chain,
            state,
        )?),
        StmtKind::IncludeOnceMark { label } => StmtKind::IncludeOnceMark { label },
        StmtKind::IncludeOnceGuard { label, body } => StmtKind::IncludeOnceGuard {
            label,
            body: resolve_isolated(body, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::Echo(expr) => StmtKind::Echo(resolve_expr(
            expr,
            base_dir,
            declared_once,
            include_chain,
            state,
        )?),
        StmtKind::Throw(expr) => StmtKind::Throw(resolve_expr(
            expr,
            base_dir,
            declared_once,
            include_chain,
            state,
        )?),
        StmtKind::ExprStmt(expr) => StmtKind::ExprStmt(resolve_expr(
            expr,
            base_dir,
            declared_once,
            include_chain,
            state,
        )?),
        StmtKind::Return(expr) => StmtKind::Return(
            expr.map(|expr| resolve_expr(expr, base_dir, declared_once, include_chain, state))
                .transpose()?,
        ),
        StmtKind::Assign { name, value } => StmtKind::Assign {
            name,
            value: resolve_expr(value, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::TypedAssign {
            type_expr,
            name,
            value,
        } => StmtKind::TypedAssign {
            type_expr,
            name,
            value: resolve_expr(value, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::ConstDecl { name, value } => StmtKind::ConstDecl {
            name,
            value: resolve_expr(value, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::ListUnpack { vars, value } => StmtKind::ListUnpack {
            vars,
            value: resolve_expr(value, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::StaticVar { name, init } => StmtKind::StaticVar {
            name,
            init: resolve_expr(init, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::ArrayAssign {
            array,
            index,
            value,
        } => StmtKind::ArrayAssign {
            array,
            index: resolve_expr(index, base_dir, declared_once, include_chain, state)?,
            value: resolve_expr(value, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::ArrayPush { array, value } => StmtKind::ArrayPush {
            array,
            value: resolve_expr(value, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::PropertyAssign {
            object,
            property,
            value,
        } => StmtKind::PropertyAssign {
            object: Box::new(resolve_expr(
                *object,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?),
            property,
            value: resolve_expr(value, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::PropertyArrayPush {
            object,
            property,
            value,
        } => StmtKind::PropertyArrayPush {
            object: Box::new(resolve_expr(
                *object,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?),
            property,
            value: resolve_expr(value, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::PropertyArrayAssign {
            object,
            property,
            index,
            value,
        } => StmtKind::PropertyArrayAssign {
            object: Box::new(resolve_expr(
                *object,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?),
            property,
            index: resolve_expr(index, base_dir, declared_once, include_chain, state)?,
            value: resolve_expr(value, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::StaticPropertyAssign {
            receiver,
            property,
            value,
        } => StmtKind::StaticPropertyAssign {
            receiver,
            property,
            value: resolve_expr(value, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::StaticPropertyArrayPush {
            receiver,
            property,
            value,
        } => StmtKind::StaticPropertyArrayPush {
            receiver,
            property,
            value: resolve_expr(value, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::StaticPropertyArrayAssign {
            receiver,
            property,
            index,
            value,
        } => StmtKind::StaticPropertyArrayAssign {
            receiver,
            property,
            index: resolve_expr(index, base_dir, declared_once, include_chain, state)?,
            value: resolve_expr(value, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::If {
            condition,
            then_body,
            elseif_clauses,
            else_body,
        } => StmtKind::If {
            condition: resolve_expr(condition, base_dir, declared_once, include_chain, state)?,
            then_body,
            elseif_clauses: elseif_clauses
                .into_iter()
                .map(|(condition, body)| {
                    Ok((
                        resolve_expr(condition, base_dir, declared_once, include_chain, state)?,
                        body,
                    ))
                })
                .collect::<Result<Vec<_>, CompileError>>()?,
            else_body,
        },
        StmtKind::While { condition, body } => StmtKind::While {
            condition: resolve_expr(condition, base_dir, declared_once, include_chain, state)?,
            body,
        },
        StmtKind::DoWhile { body, condition } => StmtKind::DoWhile {
            body,
            condition: resolve_expr(condition, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::For {
            init,
            condition,
            update,
            body,
        } => StmtKind::For {
            init: init
                .map(|stmt| {
                    resolve_stmt_exprs(*stmt, base_dir, declared_once, include_chain, state)
                        .map(Box::new)
                })
                .transpose()?,
            condition: condition
                .map(|expr| resolve_expr(expr, base_dir, declared_once, include_chain, state))
                .transpose()?,
            update: update
                .map(|stmt| {
                    resolve_stmt_exprs(*stmt, base_dir, declared_once, include_chain, state)
                        .map(Box::new)
                })
                .transpose()?,
            body,
        },
        StmtKind::Foreach {
            array,
            key_var,
            value_var,
            body,
        } => StmtKind::Foreach {
            array: resolve_expr(array, base_dir, declared_once, include_chain, state)?,
            key_var,
            value_var,
            body,
        },
        StmtKind::Switch {
            subject,
            cases,
            default,
        } => StmtKind::Switch {
            subject: resolve_expr(subject, base_dir, declared_once, include_chain, state)?,
            cases: cases
                .into_iter()
                .map(|(values, body)| {
                    Ok((
                        values
                            .into_iter()
                            .map(|value| {
                                resolve_expr(
                                    value,
                                    base_dir,
                                    declared_once,
                                    include_chain,
                                    state,
                                )
                            })
                            .collect::<Result<Vec<_>, CompileError>>()?,
                        body,
                    ))
                })
                .collect::<Result<Vec<_>, CompileError>>()?,
            default,
        },
        StmtKind::Try {
            try_body,
            catches,
            finally_body,
        } => StmtKind::Try {
            try_body,
            catches,
            finally_body,
        },
        StmtKind::FunctionDecl {
            name,
            params,
            variadic,
            return_type,
            body,
        } => StmtKind::FunctionDecl {
            name,
            params: resolve_params(params, base_dir, declared_once, include_chain, state)?,
            variadic,
            return_type,
            body,
        },
        StmtKind::ClassDecl {
            name,
            extends,
            implements,
            is_abstract,
            is_final,
            is_readonly_class,
            trait_uses,
            properties,
            methods,
        } => StmtKind::ClassDecl {
            name,
            extends,
            implements,
            is_abstract,
            is_final,
            is_readonly_class,
            trait_uses,
            properties: resolve_properties(
                properties,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?,
            methods: resolve_method_exprs(methods, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::InterfaceDecl {
            name,
            extends,
            methods,
        } => StmtKind::InterfaceDecl {
            name,
            extends,
            methods: resolve_method_exprs(methods, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::TraitDecl {
            name,
            trait_uses,
            properties,
            methods,
        } => StmtKind::TraitDecl {
            name,
            trait_uses,
            properties: resolve_properties(
                properties,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?,
            methods: resolve_method_exprs(methods, base_dir, declared_once, include_chain, state)?,
        },
        StmtKind::EnumDecl {
            name,
            backing_type,
            cases,
        } => StmtKind::EnumDecl {
            name,
            backing_type,
            cases: cases
                .into_iter()
                .map(|mut case| {
                    case.value = case
                        .value
                        .map(|expr| {
                            resolve_expr(expr, base_dir, declared_once, include_chain, state)
                        })
                        .transpose()?;
                    Ok(case)
                })
                .collect::<Result<Vec<_>, CompileError>>()?,
        },
        StmtKind::NamespaceBlock { name, body } => StmtKind::NamespaceBlock { name, body },
        StmtKind::Include {
            path,
            once,
            required,
        } => StmtKind::Include {
            path,
            once,
            required,
        },
        other @ (StmtKind::IfDef { .. }
        | StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::NamespaceDecl { .. }
        | StmtKind::UseDecl { .. }
        | StmtKind::Global { .. }
        | StmtKind::PackedClassDecl { .. }
        | StmtKind::ExternFunctionDecl { .. }
        | StmtKind::ExternClassDecl { .. }
        | StmtKind::ExternGlobalDecl { .. }) => other,
    };
    Ok(Stmt::new(kind, span))
}

