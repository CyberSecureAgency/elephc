use crate::parser::ast::{Expr, ExprKind};
use crate::span::Span;

use super::FunctionSig;

#[derive(Clone)]
pub(crate) enum PrefixArg {
    Positional(Expr),
    Spread(Expr, Span),
}

pub(crate) enum NamedParamMatch {
    Regular(usize),
    Variadic,
    Unknown,
}

pub(crate) struct DuplicateNamedParam {
    pub(crate) param_idx: usize,
}

pub(crate) struct NamedParamTracker {
    assigned: Vec<bool>,
}

impl NamedParamTracker {
    pub(crate) fn new(regular_param_count: usize) -> Self {
        Self {
            assigned: vec![false; regular_param_count],
        }
    }

    pub(crate) fn assign(
        &mut self,
        sig: &FunctionSig,
        regular_param_count: usize,
        name: &str,
        allow_unknown_named_variadic: bool,
    ) -> Result<NamedParamMatch, DuplicateNamedParam> {
        match match_named_param(sig, regular_param_count, name, allow_unknown_named_variadic) {
            NamedParamMatch::Regular(param_idx) => {
                if self.assigned.get(param_idx).copied().unwrap_or(false) {
                    Err(DuplicateNamedParam { param_idx })
                } else {
                    self.assigned[param_idx] = true;
                    Ok(NamedParamMatch::Regular(param_idx))
                }
            }
            other => Ok(other),
        }
    }
}

pub(crate) fn has_named_args(args: &[Expr]) -> bool {
    args.iter()
        .any(|arg| matches!(arg.kind, ExprKind::NamedArg { .. }))
}

pub(crate) fn regular_param_count(sig: &FunctionSig) -> usize {
    if sig.variadic.is_some() {
        sig.params.len().saturating_sub(1)
    } else {
        sig.params.len()
    }
}

pub(crate) fn named_param_index(
    sig: &FunctionSig,
    regular_param_count: usize,
    name: &str,
) -> Option<usize> {
    sig.params
        .iter()
        .take(regular_param_count)
        .position(|(param_name, _)| param_name == name)
}

pub(crate) fn match_named_param(
    sig: &FunctionSig,
    regular_param_count: usize,
    name: &str,
    allow_unknown_named_variadic: bool,
) -> NamedParamMatch {
    if let Some(param_idx) = named_param_index(sig, regular_param_count, name) {
        NamedParamMatch::Regular(param_idx)
    } else if allow_unknown_named_variadic && sig.variadic.is_some() {
        NamedParamMatch::Variadic
    } else {
        NamedParamMatch::Unknown
    }
}
