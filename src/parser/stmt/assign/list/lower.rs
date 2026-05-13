//! Purpose:
//! Lowers parsed list destructuring patterns into ordinary assignment statements.
//! Creates temporary value access expressions for positional and keyed destructuring entries.
//!
//! Called from:
//! - `crate::parser::stmt::assign::list::parse_list_unpack()`.
//!
//! Key details:
//! - Lowering preserves assignment target spans and skips invalid append targets that cannot be represented safely.

use crate::parser::ast::{Expr, ExprKind, Stmt, StmtKind};
use crate::span::Span;

use super::{ListEntry, ListPattern, ListTarget};

pub(super) fn lower_list_unpack(pattern: ListPattern, value: Expr, span: Span) -> Stmt {
    if let Some(vars) = simple_local_positional_vars(&pattern) {
        return Stmt::new(StmtKind::ListUnpack { vars, value }, span);
    }

    let mut lowerer = ListLowerer::new(span);
    let source = lowerer.bind_temp(value);
    lowerer.lower_pattern(&pattern, source);
    Stmt::new(StmtKind::Synthetic(lowerer.stmts), span)
}

fn simple_local_positional_vars(pattern: &ListPattern) -> Option<Vec<String>> {
    let mut vars = Vec::new();
    for entry in &pattern.entries {
        match entry {
            ListEntry::Target {
                key: None,
                target: ListTarget::Expr(expr),
            } => match &expr.kind {
                ExprKind::Variable(name) => vars.push(name.clone()),
                _ => return None,
            },
            _ => return None,
        }
    }
    Some(vars)
}

struct ListLowerer {
    span: Span,
    next_temp: usize,
    stmts: Vec<Stmt>,
}

impl ListLowerer {
    fn new(span: Span) -> Self {
        Self {
            span,
            next_temp: 0,
            stmts: Vec::new(),
        }
    }

    fn bind_temp(&mut self, value: Expr) -> Expr {
        let name = self.next_temp_name();
        self.stmts.push(Stmt::new(
            StmtKind::Assign {
                name: name.clone(),
                value,
            },
            self.span,
        ));
        Expr::new(ExprKind::Variable(name), self.span)
    }

    fn lower_pattern(&mut self, pattern: &ListPattern, source: Expr) {
        for (index, entry) in pattern.entries.iter().enumerate() {
            let ListEntry::Target { key, target } = entry else {
                continue;
            };
            let key_expr = key.clone().unwrap_or_else(|| Expr::int_lit(index as i64));
            let value = Expr::new(
                ExprKind::ArrayAccess {
                    array: Box::new(source.clone()),
                    index: Box::new(key_expr),
                },
                self.span,
            );
            self.lower_target(target, value);
        }
    }

    fn lower_target(&mut self, target: &ListTarget, value: Expr) {
        match target {
            ListTarget::Nested(pattern) => {
                let nested_source = self.bind_temp(value);
                self.lower_pattern(pattern, nested_source);
            }
            ListTarget::Expr(expr) => {
                if let Some(stmt) = lower_assignment_target(expr.clone(), value, self.span) {
                    self.stmts.push(stmt);
                }
            }
            ListTarget::Append(base) => {
                if let Some(stmt) = lower_append_target(base.clone(), value, self.span) {
                    self.stmts.push(stmt);
                }
            }
        }
    }

    fn next_temp_name(&mut self) -> String {
        let name = format!(
            "__elephc_list_{}_{}_{}",
            self.span.line, self.span.col, self.next_temp
        );
        self.next_temp += 1;
        name
    }
}

fn lower_assignment_target(target: Expr, value: Expr, span: Span) -> Option<Stmt> {
    let kind = match target.kind {
        ExprKind::Variable(name) => StmtKind::Assign { name, value },
        ExprKind::ArrayAccess { array, index } => match array.kind {
            ExprKind::Variable(array) => StmtKind::ArrayAssign {
                array,
                index: *index,
                value,
            },
            ExprKind::PropertyAccess { object, property } => StmtKind::PropertyArrayAssign {
                object,
                property,
                index: *index,
                value,
            },
            ExprKind::StaticPropertyAccess { receiver, property } => {
                StmtKind::StaticPropertyArrayAssign {
                    receiver,
                    property,
                    index: *index,
                    value,
                }
            }
            _ => return None,
        },
        ExprKind::PropertyAccess { object, property } => StmtKind::PropertyAssign {
            object,
            property,
            value,
        },
        ExprKind::StaticPropertyAccess { receiver, property } => StmtKind::StaticPropertyAssign {
            receiver,
            property,
            value,
        },
        _ => return None,
    };
    Some(Stmt::new(kind, span))
}

fn lower_append_target(base: Expr, value: Expr, span: Span) -> Option<Stmt> {
    let kind = match base.kind {
        ExprKind::Variable(array) => StmtKind::ArrayPush { array, value },
        ExprKind::PropertyAccess { object, property } => {
            StmtKind::PropertyArrayPush {
                object,
                property,
                value,
            }
        }
        ExprKind::StaticPropertyAccess { receiver, property } => {
            StmtKind::StaticPropertyArrayPush {
                receiver,
                property,
                value,
            }
        }
        _ => return None,
    };
    Some(Stmt::new(kind, span))
}
