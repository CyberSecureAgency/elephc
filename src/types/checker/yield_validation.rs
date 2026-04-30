use crate::errors::CompileError;
use crate::parser::ast::{Expr, ExprKind, Program, Stmt, StmtKind};

/// Returns `true` if the given function body contains any `yield` /
/// `yield from` expression at the top level — i.e. inside the function's own
/// statements, but **not** inside any nested closure (a closure with its own
/// yield is its own generator). Used by the type checker to override the
/// declared return type of a generator function to `Object("Generator")`.
pub(crate) fn body_contains_yield(body: &[Stmt]) -> bool {
    body.iter().any(stmt_contains_yield)
}

fn stmt_contains_yield(stmt: &Stmt) -> bool {
    match &stmt.kind {
        // Closures form a fresh generator scope — don't peek into them.
        StmtKind::FunctionDecl { .. } | StmtKind::ClassDecl { .. } | StmtKind::TraitDecl { .. } => false,
        StmtKind::InterfaceDecl { .. } => false,
        StmtKind::Try {
            try_body,
            catches,
            finally_body,
        } => {
            try_body.iter().any(stmt_contains_yield)
                || catches.iter().any(|c| c.body.iter().any(stmt_contains_yield))
                || finally_body
                    .as_ref()
                    .map(|f| f.iter().any(stmt_contains_yield))
                    .unwrap_or(false)
        }
        StmtKind::If {
            condition,
            then_body,
            elseif_clauses,
            else_body,
        } => {
            expr_contains_yield(condition)
                || then_body.iter().any(stmt_contains_yield)
                || elseif_clauses
                    .iter()
                    .any(|(c, b)| expr_contains_yield(c) || b.iter().any(stmt_contains_yield))
                || else_body
                    .as_ref()
                    .map(|b| b.iter().any(stmt_contains_yield))
                    .unwrap_or(false)
        }
        StmtKind::IfDef {
            then_body,
            else_body,
            ..
        } => {
            then_body.iter().any(stmt_contains_yield)
                || else_body
                    .as_ref()
                    .map(|b| b.iter().any(stmt_contains_yield))
                    .unwrap_or(false)
        }
        StmtKind::While { condition, body } | StmtKind::DoWhile { body, condition } => {
            expr_contains_yield(condition) || body.iter().any(stmt_contains_yield)
        }
        StmtKind::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_deref().map(stmt_contains_yield).unwrap_or(false)
                || condition.as_ref().map(expr_contains_yield).unwrap_or(false)
                || update.as_deref().map(stmt_contains_yield).unwrap_or(false)
                || body.iter().any(stmt_contains_yield)
        }
        StmtKind::Foreach { array, body, .. } => {
            expr_contains_yield(array) || body.iter().any(stmt_contains_yield)
        }
        StmtKind::Switch {
            subject,
            cases,
            default,
        } => {
            expr_contains_yield(subject)
                || cases.iter().any(|(vals, body)| {
                    vals.iter().any(expr_contains_yield) || body.iter().any(stmt_contains_yield)
                })
                || default
                    .as_ref()
                    .map(|d| d.iter().any(stmt_contains_yield))
                    .unwrap_or(false)
        }
        StmtKind::Synthetic(stmts) | StmtKind::NamespaceBlock { body: stmts, .. } => {
            stmts.iter().any(stmt_contains_yield)
        }
        StmtKind::Echo(e) | StmtKind::ExprStmt(e) | StmtKind::Throw(e) => expr_contains_yield(e),
        StmtKind::Assign { value, .. }
        | StmtKind::TypedAssign { value, .. }
        | StmtKind::ConstDecl { value, .. }
        | StmtKind::ListUnpack { value, .. }
        | StmtKind::StaticVar { init: value, .. } => expr_contains_yield(value),
        StmtKind::ArrayAssign { index, value, .. } => {
            expr_contains_yield(index) || expr_contains_yield(value)
        }
        StmtKind::ArrayPush { value, .. } => expr_contains_yield(value),
        StmtKind::Return(opt) => opt.as_ref().map(expr_contains_yield).unwrap_or(false),
        StmtKind::Include { path, .. } => expr_contains_yield(path),
        StmtKind::PropertyAssign { object, value, .. } => {
            expr_contains_yield(object) || expr_contains_yield(value)
        }
        StmtKind::PropertyArrayPush { object, value, .. } => {
            expr_contains_yield(object) || expr_contains_yield(value)
        }
        StmtKind::PropertyArrayAssign { object, index, value, .. } => {
            expr_contains_yield(object) || expr_contains_yield(index) || expr_contains_yield(value)
        }
        StmtKind::StaticPropertyAssign { value, .. }
        | StmtKind::StaticPropertyArrayPush { value, .. } => expr_contains_yield(value),
        StmtKind::StaticPropertyArrayAssign { index, value, .. } => {
            expr_contains_yield(index) || expr_contains_yield(value)
        }
        _ => false,
    }
}

fn expr_contains_yield(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Yield { .. } | ExprKind::YieldFrom(_) => true,
        // Don't peek into closures — their yields belong to a different generator scope.
        ExprKind::Closure { .. } => false,
        ExprKind::BinaryOp { left, right, .. } => {
            expr_contains_yield(left) || expr_contains_yield(right)
        }
        ExprKind::InstanceOf { value, .. } => expr_contains_yield(value),
        ExprKind::Negate(inner)
        | ExprKind::Not(inner)
        | ExprKind::BitNot(inner)
        | ExprKind::Throw(inner)
        | ExprKind::ErrorSuppress(inner)
        | ExprKind::Spread(inner)
        | ExprKind::Cast { expr: inner, .. }
        | ExprKind::PtrCast { expr: inner, .. } => expr_contains_yield(inner),
        ExprKind::NullCoalesce { value, default } => {
            expr_contains_yield(value) || expr_contains_yield(default)
        }
        ExprKind::FunctionCall { args, .. }
        | ExprKind::ClosureCall { args, .. }
        | ExprKind::NewObject { args, .. }
        | ExprKind::NewScopedObject { args, .. }
        | ExprKind::StaticMethodCall { args, .. } => args.iter().any(expr_contains_yield),
        ExprKind::ExprCall { callee, args } => {
            expr_contains_yield(callee) || args.iter().any(expr_contains_yield)
        }
        ExprKind::MethodCall { object, args, .. }
        | ExprKind::NullsafeMethodCall { object, args, .. } => {
            expr_contains_yield(object) || args.iter().any(expr_contains_yield)
        }
        ExprKind::ArrayLiteral(items) => items.iter().any(expr_contains_yield),
        ExprKind::ArrayLiteralAssoc(pairs) => pairs
            .iter()
            .any(|(k, v)| expr_contains_yield(k) || expr_contains_yield(v)),
        ExprKind::Match {
            subject,
            arms,
            default,
        } => {
            expr_contains_yield(subject)
                || arms.iter().any(|(patterns, value)| {
                    patterns.iter().any(expr_contains_yield) || expr_contains_yield(value)
                })
                || default.as_ref().map(|d| expr_contains_yield(d)).unwrap_or(false)
        }
        ExprKind::ArrayAccess { array, index } => {
            expr_contains_yield(array) || expr_contains_yield(index)
        }
        ExprKind::Ternary {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_contains_yield(condition)
                || expr_contains_yield(then_expr)
                || expr_contains_yield(else_expr)
        }
        ExprKind::ShortTernary { value, default } => {
            expr_contains_yield(value) || expr_contains_yield(default)
        }
        ExprKind::PropertyAccess { object, .. }
        | ExprKind::NullsafePropertyAccess { object, .. } => expr_contains_yield(object),
        ExprKind::NamedArg { value, .. } => expr_contains_yield(value),
        ExprKind::BufferNew { len, .. } => expr_contains_yield(len),
        _ => false,
    }
}

/// Walk the program AST and reject misuses of `yield` / `yield from`:
///
/// 1. Outside any function/method/closure body — yield is only valid as part
///    of a generator function.
/// 2. Inside any `try`, `catch`, or `finally` body — elephc v1 of generators
///    does not support resuming through unwinding. The error is explicit so
///    users can refactor before the codegen reports the same.
pub(crate) fn validate_yield_contexts(program: &Program) -> Vec<CompileError> {
    let mut state = State {
        function_depth: 0,
        try_finally_depth: 0,
        errors: Vec::new(),
    };
    for stmt in program {
        visit_stmt(stmt, &mut state);
    }
    state.errors
}

struct State {
    function_depth: u32,
    try_finally_depth: u32,
    errors: Vec<CompileError>,
}

fn visit_stmt(stmt: &Stmt, st: &mut State) {
    match &stmt.kind {
        StmtKind::FunctionDecl { body, .. } => {
            st.function_depth += 1;
            for s in body {
                visit_stmt(s, st);
            }
            st.function_depth -= 1;
        }
        StmtKind::ClassDecl { methods, .. } | StmtKind::TraitDecl { methods, .. } => {
            for m in methods {
                if !m.has_body {
                    continue;
                }
                st.function_depth += 1;
                for s in &m.body {
                    visit_stmt(s, st);
                }
                st.function_depth -= 1;
            }
        }
        StmtKind::InterfaceDecl { .. } => {}
        StmtKind::Try {
            try_body,
            catches,
            finally_body,
        } => {
            // The `try` body itself, plus any `catch`/`finally`, all forbid yield.
            st.try_finally_depth += 1;
            for s in try_body {
                visit_stmt(s, st);
            }
            for c in catches {
                for s in &c.body {
                    visit_stmt(s, st);
                }
            }
            if let Some(fin) = finally_body {
                for s in fin {
                    visit_stmt(s, st);
                }
            }
            st.try_finally_depth -= 1;
        }
        StmtKind::If {
            condition,
            then_body,
            elseif_clauses,
            else_body,
        } => {
            visit_expr(condition, st);
            for s in then_body {
                visit_stmt(s, st);
            }
            for (cond, body) in elseif_clauses {
                visit_expr(cond, st);
                for s in body {
                    visit_stmt(s, st);
                }
            }
            if let Some(else_body) = else_body {
                for s in else_body {
                    visit_stmt(s, st);
                }
            }
        }
        StmtKind::IfDef {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                visit_stmt(s, st);
            }
            if let Some(else_body) = else_body {
                for s in else_body {
                    visit_stmt(s, st);
                }
            }
        }
        StmtKind::While { condition, body } | StmtKind::DoWhile { body, condition } => {
            visit_expr(condition, st);
            for s in body {
                visit_stmt(s, st);
            }
        }
        StmtKind::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                visit_stmt(init, st);
            }
            if let Some(cond) = condition {
                visit_expr(cond, st);
            }
            if let Some(up) = update {
                visit_stmt(up, st);
            }
            for s in body {
                visit_stmt(s, st);
            }
        }
        StmtKind::Foreach {
            array, body, ..
        } => {
            visit_expr(array, st);
            for s in body {
                visit_stmt(s, st);
            }
        }
        StmtKind::Switch {
            subject,
            cases,
            default,
        } => {
            visit_expr(subject, st);
            for (vals, body) in cases {
                for v in vals {
                    visit_expr(v, st);
                }
                for s in body {
                    visit_stmt(s, st);
                }
            }
            if let Some(default) = default {
                for s in default {
                    visit_stmt(s, st);
                }
            }
        }
        StmtKind::Synthetic(stmts) | StmtKind::NamespaceBlock { body: stmts, .. } => {
            for s in stmts {
                visit_stmt(s, st);
            }
        }
        StmtKind::Echo(e) | StmtKind::ExprStmt(e) | StmtKind::Throw(e) => visit_expr(e, st),
        StmtKind::Assign { value, .. }
        | StmtKind::TypedAssign { value, .. }
        | StmtKind::ConstDecl { value, .. }
        | StmtKind::ListUnpack { value, .. }
        | StmtKind::StaticVar { init: value, .. } => visit_expr(value, st),
        StmtKind::ArrayAssign { index, value, .. } => {
            visit_expr(index, st);
            visit_expr(value, st);
        }
        StmtKind::ArrayPush { value, .. } => visit_expr(value, st),
        StmtKind::Return(opt) => {
            if let Some(e) = opt {
                visit_expr(e, st);
            }
        }
        StmtKind::Include { path, .. } => visit_expr(path, st),
        StmtKind::PropertyAssign { object, value, .. } => {
            visit_expr(object, st);
            visit_expr(value, st);
        }
        StmtKind::PropertyArrayPush { object, value, .. } => {
            visit_expr(object, st);
            visit_expr(value, st);
        }
        StmtKind::PropertyArrayAssign { object, index, value, .. } => {
            visit_expr(object, st);
            visit_expr(index, st);
            visit_expr(value, st);
        }
        StmtKind::StaticPropertyAssign { value, .. }
        | StmtKind::StaticPropertyArrayPush { value, .. } => visit_expr(value, st),
        StmtKind::StaticPropertyArrayAssign { index, value, .. } => {
            visit_expr(index, st);
            visit_expr(value, st);
        }
        // Statements that don't carry expressions or sub-bodies for yield checks.
        StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::NamespaceDecl { .. }
        | StmtKind::UseDecl { .. }
        | StmtKind::EnumDecl { .. }
        | StmtKind::PackedClassDecl { .. }
        | StmtKind::Global { .. }
        | StmtKind::ExternFunctionDecl { .. }
        | StmtKind::ExternClassDecl { .. }
        | StmtKind::ExternGlobalDecl { .. }
        | StmtKind::IncludeOnceMark { .. }
        | StmtKind::IncludeOnceGuard { .. }
        | StmtKind::FunctionVariantGroup { .. }
        | StmtKind::FunctionVariantMark { .. } => {}
    }
}

fn visit_expr(expr: &Expr, st: &mut State) {
    match &expr.kind {
        ExprKind::Yield { key, value } => {
            check_yield_context(expr.span, st);
            if let Some(k) = key {
                visit_expr(k, st);
            }
            if let Some(v) = value {
                visit_expr(v, st);
            }
        }
        ExprKind::YieldFrom(inner) => {
            check_yield_context(expr.span, st);
            visit_expr(inner, st);
        }
        ExprKind::Closure { body, .. } => {
            // Closures introduce a fresh function scope. A yield inside a
            // closure refers to that closure (which would make it a generator
            // closure — currently unsupported in v1, but lex/parse/typecheck
            // accept the syntax). Reset try-depth for the new scope.
            let saved_try = st.try_finally_depth;
            st.try_finally_depth = 0;
            st.function_depth += 1;
            for s in body {
                visit_stmt(s, st);
            }
            st.function_depth -= 1;
            st.try_finally_depth = saved_try;
        }
        // Expressions with sub-expressions to recurse into.
        ExprKind::BinaryOp { left, right, .. } => {
            visit_expr(left, st);
            visit_expr(right, st);
        }
        ExprKind::InstanceOf { value, .. } => visit_expr(value, st),
        ExprKind::Negate(inner)
        | ExprKind::Not(inner)
        | ExprKind::BitNot(inner)
        | ExprKind::Throw(inner)
        | ExprKind::ErrorSuppress(inner)
        | ExprKind::Spread(inner)
        | ExprKind::Cast { expr: inner, .. }
        | ExprKind::PtrCast { expr: inner, .. } => visit_expr(inner, st),
        ExprKind::NullCoalesce { value, default } => {
            visit_expr(value, st);
            visit_expr(default, st);
        }
        ExprKind::FunctionCall { args, .. }
        | ExprKind::ClosureCall { args, .. }
        | ExprKind::NewObject { args, .. }
        | ExprKind::NewScopedObject { args, .. }
        | ExprKind::StaticMethodCall { args, .. } => {
            for a in args {
                visit_expr(a, st);
            }
        }
        ExprKind::ExprCall { callee, args } => {
            visit_expr(callee, st);
            for a in args {
                visit_expr(a, st);
            }
        }
        ExprKind::MethodCall { object, args, .. }
        | ExprKind::NullsafeMethodCall { object, args, .. } => {
            visit_expr(object, st);
            for a in args {
                visit_expr(a, st);
            }
        }
        ExprKind::ArrayLiteral(items) => {
            for it in items {
                visit_expr(it, st);
            }
        }
        ExprKind::ArrayLiteralAssoc(pairs) => {
            for (k, v) in pairs {
                visit_expr(k, st);
                visit_expr(v, st);
            }
        }
        ExprKind::Match {
            subject,
            arms,
            default,
        } => {
            visit_expr(subject, st);
            for (patterns, value) in arms {
                for p in patterns {
                    visit_expr(p, st);
                }
                visit_expr(value, st);
            }
            if let Some(d) = default {
                visit_expr(d, st);
            }
        }
        ExprKind::ArrayAccess { array, index } => {
            visit_expr(array, st);
            visit_expr(index, st);
        }
        ExprKind::Ternary {
            condition,
            then_expr,
            else_expr,
        } => {
            visit_expr(condition, st);
            visit_expr(then_expr, st);
            visit_expr(else_expr, st);
        }
        ExprKind::ShortTernary { value, default } => {
            visit_expr(value, st);
            visit_expr(default, st);
        }
        ExprKind::PropertyAccess { object, .. }
        | ExprKind::NullsafePropertyAccess { object, .. } => visit_expr(object, st),
        ExprKind::NamedArg { value, .. } => visit_expr(value, st),
        ExprKind::BufferNew { len, .. } => visit_expr(len, st),
        // Leaves
        ExprKind::StringLiteral(_)
        | ExprKind::IntLiteral(_)
        | ExprKind::FloatLiteral(_)
        | ExprKind::BoolLiteral(_)
        | ExprKind::Null
        | ExprKind::Variable(_)
        | ExprKind::PreIncrement(_)
        | ExprKind::PostIncrement(_)
        | ExprKind::PreDecrement(_)
        | ExprKind::PostDecrement(_)
        | ExprKind::ConstRef(_)
        | ExprKind::EnumCase { .. }
        | ExprKind::This
        | ExprKind::FirstClassCallable(_)
        | ExprKind::StaticPropertyAccess { .. }
        | ExprKind::ClassConstant { .. }
        | ExprKind::MagicConstant(_) => {}
        ExprKind::Print(inner) => visit_expr(inner, st),
        ExprKind::Assignment { target, value, .. } => {
            visit_expr(target, st);
            visit_expr(value, st);
        }
    }
}

fn check_yield_context(span: crate::span::Span, st: &mut State) {
    if st.function_depth == 0 {
        st.errors.push(CompileError::new(
            span,
            "yield can only be used inside a function or method body",
        ));
    } else if st.try_finally_depth > 0 {
        st.errors.push(CompileError::new(
            span,
            "yield inside try/catch/finally is not yet supported",
        ));
    }
}
