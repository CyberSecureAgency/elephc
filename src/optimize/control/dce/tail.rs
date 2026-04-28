use super::*;
use super::state::TailSinkTarget;

pub(super) fn append_tail_to_fallthrough_path(mut body: Vec<Stmt>, tail: Vec<Stmt>) -> Vec<Stmt> {
    if block_reaches_following_stmt(&body) {
        body.extend(tail);
    }
    body
}

fn block_matches_tail_target(body: &[Stmt], target: TailSinkTarget) -> bool {
    matches!(
        (block_terminal_effect(body), target),
        (TerminalEffect::FallsThrough, TailSinkTarget::FallsThrough)
            | (TerminalEffect::Breaks, TailSinkTarget::Breaks)
    )
}

pub(super) fn sink_tail_into_terminal_path(
    mut body: Vec<Stmt>,
    tail: Vec<Stmt>,
    target: TailSinkTarget,
) -> Vec<Stmt> {
    let Some(stmt) = body.pop() else {
        return tail;
    };

    let rewritten = sink_tail_into_terminal_stmt(stmt, tail, target);
    body.extend(rewritten);
    body
}

fn sink_tail_into_terminal_stmt(stmt: Stmt, tail: Vec<Stmt>, target: TailSinkTarget) -> Vec<Stmt> {
    let span = stmt.span;
    match stmt.kind {
        StmtKind::If {
            condition,
            then_body,
            elseif_clauses,
            else_body,
        } => {
            let rewrite_branch = |body: Vec<Stmt>, target: TailSinkTarget, tail: &Vec<Stmt>| {
                if block_matches_tail_target(&body, target) {
                    sink_tail_into_terminal_path(body, tail.clone(), target)
                } else {
                    body
                }
            };
            let then_body = rewrite_branch(then_body, target, &tail);
            let elseif_clauses: Vec<_> = elseif_clauses
                .into_iter()
                .map(|(condition, body)| (condition, rewrite_branch(body, target, &tail)))
                .collect();
            let else_body = else_body.map(|body| rewrite_branch(body, target, &tail));
            vec![Stmt::new(
                StmtKind::If {
                    condition,
                    then_body,
                    elseif_clauses,
                    else_body,
                },
                span,
            )]
        }
        StmtKind::IfDef {
            symbol,
            then_body,
            else_body,
        } => {
            let then_body = if block_matches_tail_target(&then_body, target) {
                sink_tail_into_terminal_path(then_body, tail.clone(), target)
            } else {
                then_body
            };
            let else_body = else_body.map(|body| {
                if block_matches_tail_target(&body, target) {
                    sink_tail_into_terminal_path(body, tail.clone(), target)
                } else {
                    body
                }
            });
            vec![Stmt::new(
                StmtKind::IfDef {
                    symbol,
                    then_body,
                    else_body,
                },
                span,
            )]
        }
        StmtKind::Try {
            try_body,
            catches,
            finally_body,
        } => {
            let try_body = if block_matches_tail_target(&try_body, target) {
                sink_tail_into_terminal_path(try_body, tail.clone(), target)
            } else {
                try_body
            };
            let catches = catches
                .into_iter()
                .map(|catch| crate::parser::ast::CatchClause {
                    body: if block_matches_tail_target(&catch.body, target) {
                        sink_tail_into_terminal_path(catch.body, tail.clone(), target)
                    } else {
                        catch.body
                    },
                    ..catch
                })
                .collect();
            vec![Stmt::new(
                StmtKind::Try {
                    try_body,
                    catches,
                    finally_body,
                },
                span,
            )]
        }
        _ if matches!(target, TailSinkTarget::FallsThrough)
            && matches!(stmt_terminal_effect(&stmt), TerminalEffect::FallsThrough) =>
        {
            let mut stmts = vec![stmt];
            stmts.extend(tail);
            stmts
        }
        StmtKind::Break if matches!(target, TailSinkTarget::Breaks) => {
            let mut stmts = tail;
            if block_reaches_following_stmt(&stmts) {
                stmts.push(Stmt::new(StmtKind::Break, span));
            }
            stmts
        }
        _ => vec![stmt],
    }
}
