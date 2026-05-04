use crate::errors::CompileError;
use crate::parser::ast::{CatchClause, Expr, StaticReceiver, Stmt, TypeExpr};

use super::list::resolve_stmt_list;
use super::super::expressions::resolve_expr;
use super::super::names::{resolve_special_or_class_name, resolve_type_expr};
use super::super::{resolved_name, Imports, Symbols};

#[derive(Clone, Copy)]
pub(super) struct ResolveContext<'a> {
    namespace: Option<&'a str>,
    imports: &'a Imports,
    symbols: &'a Symbols,
}

impl<'a> ResolveContext<'a> {
    pub(super) fn new(
        namespace: Option<&'a str>,
        imports: &'a Imports,
        symbols: &'a Symbols,
    ) -> Self {
        Self {
            namespace,
            imports,
            symbols,
        }
    }

    pub(super) fn expr(&self, expr: &Expr) -> Expr {
        resolve_expr(expr, self.namespace, self.imports, self.symbols)
    }

    pub(super) fn stmt_list(&self, stmts: &[Stmt]) -> Result<Vec<Stmt>, CompileError> {
        resolve_stmt_list(stmts, self.namespace, self.imports, self.symbols)
    }

    pub(super) fn one_stmt(&self, stmt: &Stmt) -> Result<Stmt, CompileError> {
        let mut stmts = self.stmt_list(std::slice::from_ref(stmt))?;
        Ok(stmts.remove(0))
    }

    pub(super) fn catch_clause(
        &self,
        catch_clause: &CatchClause,
    ) -> Result<CatchClause, CompileError> {
        Ok(CatchClause {
            exception_types: catch_clause
                .exception_types
                .iter()
                .map(|name| {
                    resolved_name(resolve_special_or_class_name(
                        name,
                        self.namespace,
                        self.imports,
                        self.symbols,
                    ))
                })
                .collect(),
            variable: catch_clause.variable.clone(),
            body: self.stmt_list(&catch_clause.body)?,
        })
    }

    pub(super) fn type_expr(&self, type_expr: &TypeExpr) -> TypeExpr {
        resolve_type_expr(type_expr, self.namespace, self.imports, self.symbols)
    }

    pub(super) fn static_receiver(&self, receiver: &StaticReceiver) -> StaticReceiver {
        match receiver {
            StaticReceiver::Named(name) => StaticReceiver::Named(resolved_name(
                resolve_special_or_class_name(name, self.namespace, self.imports, self.symbols),
            )),
            _ => receiver.clone(),
        }
    }
}

pub(in crate::name_resolver) fn resolve_params(
    params: &[(String, Option<TypeExpr>, Option<Expr>, bool)],
    current_namespace: Option<&str>,
    imports: &Imports,
    symbols: &Symbols,
) -> Vec<(String, Option<TypeExpr>, Option<Expr>, bool)> {
    let ctx = ResolveContext::new(current_namespace, imports, symbols);
    params
        .iter()
        .map(|(name, type_ann, default, is_ref)| {
            (
                name.clone(),
                type_ann.as_ref().map(|ty| ctx.type_expr(ty)),
                default.as_ref().map(|expr| ctx.expr(expr)),
                *is_ref,
            )
        })
        .collect()
}
