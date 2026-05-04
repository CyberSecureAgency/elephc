use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::errors::CompileError;
use crate::parser::ast::{CallableTarget, ClassMethod, Expr, ExprKind};

use super::engine::resolve_isolated;
use super::state::ResolveState;

pub(super) fn resolve_expr(
    expr: Expr,
    base_dir: &Path,
    declared_once: &mut HashSet<PathBuf>,
    include_chain: &mut Vec<PathBuf>,
    state: &ResolveState,
) -> Result<Expr, CompileError> {
    let span = expr.span;
    let kind = match expr.kind {
        ExprKind::BinaryOp { left, op, right } => ExprKind::BinaryOp {
            left: Box::new(resolve_expr(*left, base_dir, declared_once, include_chain, state)?),
            op,
            right: Box::new(resolve_expr(*right, base_dir, declared_once, include_chain, state)?),
        },
        ExprKind::InstanceOf { value, target } => ExprKind::InstanceOf {
            value: Box::new(resolve_expr(*value, base_dir, declared_once, include_chain, state)?),
            target,
        },
        ExprKind::Negate(inner) => ExprKind::Negate(Box::new(resolve_expr(
            *inner,
            base_dir,
            declared_once,
            include_chain,
            state,
        )?)),
        ExprKind::Not(inner) => ExprKind::Not(Box::new(resolve_expr(
            *inner,
            base_dir,
            declared_once,
            include_chain,
            state,
        )?)),
        ExprKind::BitNot(inner) => ExprKind::BitNot(Box::new(resolve_expr(
            *inner,
            base_dir,
            declared_once,
            include_chain,
            state,
        )?)),
        ExprKind::Throw(inner) => ExprKind::Throw(Box::new(resolve_expr(
            *inner,
            base_dir,
            declared_once,
            include_chain,
            state,
        )?)),
        ExprKind::ErrorSuppress(inner) => ExprKind::ErrorSuppress(Box::new(resolve_expr(
            *inner,
            base_dir,
            declared_once,
            include_chain,
            state,
        )?)),
        ExprKind::Print(inner) => ExprKind::Print(Box::new(resolve_expr(
            *inner,
            base_dir,
            declared_once,
            include_chain,
            state,
        )?)),
        ExprKind::NullCoalesce { value, default } => ExprKind::NullCoalesce {
            value: Box::new(resolve_expr(*value, base_dir, declared_once, include_chain, state)?),
            default: Box::new(resolve_expr(*default, base_dir, declared_once, include_chain, state)?),
        },
        ExprKind::Assignment {
            target,
            value,
            result_target,
            prelude,
            conditional_value_temp,
        } => ExprKind::Assignment {
            target: Box::new(resolve_expr(*target, base_dir, declared_once, include_chain, state)?),
            value: Box::new(resolve_expr(*value, base_dir, declared_once, include_chain, state)?),
            result_target: result_target
                .map(|target| resolve_expr(*target, base_dir, declared_once, include_chain, state))
                .transpose()?
                .map(Box::new),
            prelude: resolve_isolated(prelude, base_dir, declared_once, include_chain, state)?,
            conditional_value_temp,
        },
        ExprKind::FunctionCall { name, args } => ExprKind::FunctionCall {
            name,
            args: resolve_exprs(args, base_dir, declared_once, include_chain, state)?,
        },
        ExprKind::ArrayLiteral(items) => ExprKind::ArrayLiteral(resolve_exprs(
            items,
            base_dir,
            declared_once,
            include_chain,
            state,
        )?),
        ExprKind::ArrayLiteralAssoc(entries) => ExprKind::ArrayLiteralAssoc(
            entries
                .into_iter()
                .map(|(key, value)| {
                    Ok((
                        resolve_expr(key, base_dir, declared_once, include_chain, state)?,
                        resolve_expr(value, base_dir, declared_once, include_chain, state)?,
                    ))
                })
                .collect::<Result<Vec<_>, CompileError>>()?,
        ),
        ExprKind::Match {
            subject,
            arms,
            default,
        } => ExprKind::Match {
            subject: Box::new(resolve_expr(
                *subject,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?),
            arms: arms
                .into_iter()
                .map(|(patterns, value)| {
                    Ok((
                        resolve_exprs(patterns, base_dir, declared_once, include_chain, state)?,
                        resolve_expr(value, base_dir, declared_once, include_chain, state)?,
                    ))
                })
                .collect::<Result<Vec<_>, CompileError>>()?,
            default: default
                .map(|expr| resolve_expr(*expr, base_dir, declared_once, include_chain, state))
                .transpose()?
                .map(Box::new),
        },
        ExprKind::ArrayAccess { array, index } => ExprKind::ArrayAccess {
            array: Box::new(resolve_expr(*array, base_dir, declared_once, include_chain, state)?),
            index: Box::new(resolve_expr(*index, base_dir, declared_once, include_chain, state)?),
        },
        ExprKind::Ternary {
            condition,
            then_expr,
            else_expr,
        } => ExprKind::Ternary {
            condition: Box::new(resolve_expr(
                *condition,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?),
            then_expr: Box::new(resolve_expr(
                *then_expr,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?),
            else_expr: Box::new(resolve_expr(
                *else_expr,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?),
        },
        ExprKind::ShortTernary { value, default } => ExprKind::ShortTernary {
            value: Box::new(resolve_expr(*value, base_dir, declared_once, include_chain, state)?),
            default: Box::new(resolve_expr(*default, base_dir, declared_once, include_chain, state)?),
        },
        ExprKind::Cast { target, expr } => ExprKind::Cast {
            target,
            expr: Box::new(resolve_expr(*expr, base_dir, declared_once, include_chain, state)?),
        },
        ExprKind::Closure {
            params,
            variadic,
            return_type,
            body,
            is_arrow,
            is_static,
            captures,
        } => ExprKind::Closure {
            params: resolve_params(params, base_dir, declared_once, include_chain, state)?,
            variadic,
            return_type,
            body: resolve_isolated(body, base_dir, declared_once, include_chain, state)?,
            is_arrow,
            is_static,
            captures,
        },
        ExprKind::NamedArg { name, value } => ExprKind::NamedArg {
            name,
            value: Box::new(resolve_expr(*value, base_dir, declared_once, include_chain, state)?),
        },
        ExprKind::Spread(inner) => ExprKind::Spread(Box::new(resolve_expr(
            *inner,
            base_dir,
            declared_once,
            include_chain,
            state,
        )?)),
        ExprKind::ClosureCall { var, args } => ExprKind::ClosureCall {
            var,
            args: resolve_exprs(args, base_dir, declared_once, include_chain, state)?,
        },
        ExprKind::ExprCall { callee, args } => ExprKind::ExprCall {
            callee: Box::new(resolve_expr(
                *callee,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?),
            args: resolve_exprs(args, base_dir, declared_once, include_chain, state)?,
        },
        ExprKind::NewObject { class_name, args } => ExprKind::NewObject {
            class_name,
            args: resolve_exprs(args, base_dir, declared_once, include_chain, state)?,
        },
        ExprKind::PropertyAccess { object, property } => ExprKind::PropertyAccess {
            object: Box::new(resolve_expr(
                *object,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?),
            property,
        },
        ExprKind::NullsafePropertyAccess { object, property } => {
            ExprKind::NullsafePropertyAccess {
                object: Box::new(resolve_expr(
                    *object,
                    base_dir,
                    declared_once,
                    include_chain,
                    state,
                )?),
                property,
            }
        }
        ExprKind::MethodCall { object, method, args } => ExprKind::MethodCall {
            object: Box::new(resolve_expr(
                *object,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?),
            method,
            args: resolve_exprs(args, base_dir, declared_once, include_chain, state)?,
        },
        ExprKind::NullsafeMethodCall {
            object,
            method,
            args,
        } => ExprKind::NullsafeMethodCall {
            object: Box::new(resolve_expr(
                *object,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?),
            method,
            args: resolve_exprs(args, base_dir, declared_once, include_chain, state)?,
        },
        ExprKind::StaticMethodCall {
            receiver,
            method,
            args,
        } => ExprKind::StaticMethodCall {
            receiver,
            method,
            args: resolve_exprs(args, base_dir, declared_once, include_chain, state)?,
        },
        ExprKind::FirstClassCallable(target) => {
            ExprKind::FirstClassCallable(resolve_callable_target(
                target,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?)
        }
        ExprKind::PtrCast { target_type, expr } => ExprKind::PtrCast {
            target_type,
            expr: Box::new(resolve_expr(*expr, base_dir, declared_once, include_chain, state)?),
        },
        ExprKind::BufferNew { element_type, len } => ExprKind::BufferNew {
            element_type,
            len: Box::new(resolve_expr(*len, base_dir, declared_once, include_chain, state)?),
        },
        ExprKind::NewScopedObject { receiver, args } => ExprKind::NewScopedObject {
            receiver,
            args: resolve_exprs(args, base_dir, declared_once, include_chain, state)?,
        },
        other => other,
    };
    Ok(Expr::new(kind, span))
}

fn resolve_exprs(
    exprs: Vec<Expr>,
    base_dir: &Path,
    declared_once: &mut HashSet<PathBuf>,
    include_chain: &mut Vec<PathBuf>,
    state: &ResolveState,
) -> Result<Vec<Expr>, CompileError> {
    exprs
        .into_iter()
        .map(|expr| resolve_expr(expr, base_dir, declared_once, include_chain, state))
        .collect()
}

pub(super) fn resolve_params(
    params: Vec<(String, Option<crate::parser::ast::TypeExpr>, Option<Expr>, bool)>,
    base_dir: &Path,
    declared_once: &mut HashSet<PathBuf>,
    include_chain: &mut Vec<PathBuf>,
    state: &ResolveState,
) -> Result<Vec<(String, Option<crate::parser::ast::TypeExpr>, Option<Expr>, bool)>, CompileError> {
    params
        .into_iter()
        .map(|(name, type_expr, default, is_ref)| {
            Ok((
                name,
                type_expr,
                default
                    .map(|expr| resolve_expr(expr, base_dir, declared_once, include_chain, state))
                    .transpose()?,
                is_ref,
            ))
        })
        .collect()
}

pub(super) fn resolve_properties(
    properties: Vec<crate::parser::ast::ClassProperty>,
    base_dir: &Path,
    declared_once: &mut HashSet<PathBuf>,
    include_chain: &mut Vec<PathBuf>,
    state: &ResolveState,
) -> Result<Vec<crate::parser::ast::ClassProperty>, CompileError> {
    properties
        .into_iter()
        .map(|mut property| {
            property.default = property
                .default
                .map(|expr| resolve_expr(expr, base_dir, declared_once, include_chain, state))
                .transpose()?;
            Ok(property)
        })
        .collect()
}

pub(super) fn resolve_method_exprs(
    methods: Vec<ClassMethod>,
    base_dir: &Path,
    declared_once: &mut HashSet<PathBuf>,
    include_chain: &mut Vec<PathBuf>,
    state: &ResolveState,
) -> Result<Vec<ClassMethod>, CompileError> {
    methods
        .into_iter()
        .map(|mut method| {
            method.params = resolve_params(method.params, base_dir, declared_once, include_chain, state)?;
            Ok(method)
        })
        .collect()
}

fn resolve_callable_target(
    target: CallableTarget,
    base_dir: &Path,
    declared_once: &mut HashSet<PathBuf>,
    include_chain: &mut Vec<PathBuf>,
    state: &ResolveState,
) -> Result<CallableTarget, CompileError> {
    Ok(match target {
        CallableTarget::Function(name) => CallableTarget::Function(name),
        CallableTarget::StaticMethod { receiver, method } => {
            CallableTarget::StaticMethod { receiver, method }
        }
        CallableTarget::Method { object, method } => CallableTarget::Method {
            object: Box::new(resolve_expr(
                *object,
                base_dir,
                declared_once,
                include_chain,
                state,
            )?),
            method,
        },
    })
}
