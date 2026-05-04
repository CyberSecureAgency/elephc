use std::collections::HashMap;

use crate::names::{Name, NameKind};
use crate::parser::ast::{Stmt, StmtKind, UseKind};

#[derive(Clone, Default)]
pub(super) struct ResolveState {
    pub(super) constants: HashMap<String, String>,
    pub(super) namespace: Option<String>,
    pub(super) const_imports: HashMap<String, String>,
}

pub(super) fn resolve_constant_ref(name: &Name, state: &ResolveState) -> Option<String> {
    constant_lookup_candidates(name, state)
        .into_iter()
        .find_map(|candidate| state.constants.get(&candidate).cloned())
}

fn constant_lookup_candidates(name: &Name, state: &ResolveState) -> Vec<String> {
    if name.is_fully_qualified() {
        return vec![name.as_canonical()];
    }

    if name.is_unqualified() {
        if let Some(alias) = name
            .last_segment()
            .and_then(|segment| state.const_imports.get(segment))
        {
            return vec![alias.clone()];
        }

        let raw = name.as_canonical();
        if let Some(namespace) = state.namespace.as_deref() {
            if !namespace.is_empty() {
                return vec![format!("{}\\{}", namespace, raw), raw];
            }
        }
        return vec![raw];
    }

    if let Some(first) = name.parts.first() {
        if let Some(alias) = state.const_imports.get(first) {
            let suffix = &name.parts[1..];
            if suffix.is_empty() {
                return vec![alias.clone()];
            }
            return vec![format!("{}\\{}", alias, suffix.join("\\"))];
        }
    }

    let raw = name.as_canonical();
    if name.kind == NameKind::Qualified {
        if let Some(namespace) = state.namespace.as_deref() {
            if !namespace.is_empty() {
                return vec![format!("{}\\{}", namespace, raw)];
            }
        }
    }
    vec![raw]
}

pub(super) fn normalize_defined_constant_name(name: &str) -> String {
    name.trim_start_matches('\\').to_string()
}

pub(super) fn namespace_string(name: &Option<Name>) -> String {
    name.as_ref().map(Name::as_canonical).unwrap_or_default()
}

pub(super) fn register_const_imports(state: &mut ResolveState, stmt: &Stmt) {
    let StmtKind::UseDecl { imports } = &stmt.kind else {
        return;
    };
    for item in imports {
        if item.kind == UseKind::Const {
            state.const_imports.insert(
                item.alias.clone(),
                normalize_defined_constant_name(&item.name.as_canonical()),
            );
        }
    }
}

pub(super) fn is_define_call_name(name: &Name) -> bool {
    matches!(name.kind, NameKind::Unqualified | NameKind::FullyQualified)
        && name.parts.len() == 1
        && name.parts[0] == "define"
}
