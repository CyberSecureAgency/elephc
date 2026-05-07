use crate::codegen::context::{Context, DeferredFiberWrapper};
use crate::parser::ast::{Expr, ExprKind, Stmt, StmtKind};
use crate::types::{FunctionSig, PhpType};

pub(super) fn prepare_fiber_wrapper(callable_expr: &Expr, ctx: &mut Context) -> Option<String> {
    let (mut sig, visible_param_count) = match &callable_expr.kind {
        ExprKind::Closure {
            params,
            variadic,
            body,
            ..
        } => {
            let visible_param_count = params.len() + usize::from(variadic.is_some());
            let no_terminal_return = !closure_body_has_return(body);
            let deferred = ctx.deferred_closures.last_mut()?;
            adapt_fiber_closure_sig(&mut deferred.sig, visible_param_count, no_terminal_return);
            (deferred.sig.clone(), visible_param_count)
        }
        ExprKind::Variable(name) => {
            let captures = ctx.closure_captures.get(name).cloned().unwrap_or_default();
            let mut sig = ctx.closure_sigs.get(name).cloned()?;
            let visible_param_count = sig.params.len().saturating_sub(captures.len());
            if let Some(deferred) = ctx.deferred_closures.iter_mut().rev().find(|deferred| {
                deferred.sig.params == sig.params && deferred.captures == captures
            }) {
                let no_terminal_return = !closure_body_has_return(&deferred.body);
                adapt_fiber_closure_sig(
                    &mut deferred.sig,
                    visible_param_count,
                    no_terminal_return,
                );
                sig = deferred.sig.clone();
            } else {
                adapt_fiber_closure_sig(&mut sig, visible_param_count, false);
            }
            ctx.closure_sigs.insert(name.clone(), sig.clone());
            (sig, visible_param_count)
        }
        _ => return None,
    };

    adapt_fiber_closure_sig(&mut sig, visible_param_count, false);
    let label = ctx.next_label("fiber_entry_wrapper");
    ctx.deferred_fiber_wrappers.push(DeferredFiberWrapper {
        label: label.clone(),
        sig,
        visible_param_count,
    });
    Some(label)
}

fn adapt_fiber_closure_sig(
    sig: &mut FunctionSig,
    visible_param_count: usize,
    no_terminal_return: bool,
) {
    for i in 0..visible_param_count.min(sig.params.len()) {
        let declared = sig.declared_params.get(i).copied().unwrap_or(false);
        let by_ref = sig.ref_params.get(i).copied().unwrap_or(false);
        if !declared && !by_ref {
            sig.params[i].1 = PhpType::Mixed;
        }
    }
    if no_terminal_return && !sig.declared_return {
        sig.return_type = PhpType::Void;
    }
}

fn closure_body_has_return(body: &[Stmt]) -> bool {
    body.iter().any(stmt_has_return)
}

fn stmt_has_return(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return(_) => true,
        StmtKind::Synthetic(stmts) => closure_body_has_return(stmts),
        StmtKind::If {
            then_body,
            elseif_clauses,
            else_body,
            ..
        } => {
            closure_body_has_return(then_body)
                || elseif_clauses
                    .iter()
                    .any(|(_, body)| closure_body_has_return(body))
                || else_body
                    .as_ref()
                    .is_some_and(|body| closure_body_has_return(body))
        }
        StmtKind::While { body, .. }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::For { body, .. }
        | StmtKind::Foreach { body, .. } => closure_body_has_return(body),
        StmtKind::Try {
            try_body,
            catches,
            finally_body,
        } => {
            closure_body_has_return(try_body)
                || catches
                    .iter()
                    .any(|catch_clause| closure_body_has_return(&catch_clause.body))
                || finally_body
                    .as_ref()
                    .is_some_and(|body| closure_body_has_return(body))
        }
        StmtKind::Switch { cases, default, .. } => {
            cases.iter().any(|(_, body)| closure_body_has_return(body))
                || default
                    .as_ref()
                    .is_some_and(|body| closure_body_has_return(body))
        }
        _ => false,
    }
}
