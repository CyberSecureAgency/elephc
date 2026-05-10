//! Purpose:
//! Walks class properties and methods during magic-constant substitution.
//! Applies expression and statement walkers to defaults, bodies, and promoted-property assignments.
//!
//! Called from:
//! - `crate::magic_constants::walker::stmts` and trait binding passes.
//!
//! Key details:
//! - Member traversal preserves declaration metadata while updating only magic-constant-bearing children.

use crate::parser::ast::{ClassMethod, ClassProperty};

use super::exprs::walk_expr;
use super::stmts::walk_program;
use super::Pass;

pub(in crate::magic_constants) fn walk_class_property<P: Pass>(
    prop: ClassProperty,
    pass: &mut P,
) -> ClassProperty {
    ClassProperty {
        default: prop.default.map(|e| walk_expr(e, pass)),
        ..prop
    }
}

pub(in crate::magic_constants) fn walk_class_method<P: Pass>(
    method: ClassMethod,
    pass: &mut P,
) -> ClassMethod {
    pass.enter_method(&method.name);
    let new_params = method
        .params
        .into_iter()
        .map(|(n, t, default, by_ref)| (n, t, default.map(|d| walk_expr(d, pass)), by_ref))
        .collect();
    let new_body = walk_program(method.body, pass);
    pass.leave_method();
    ClassMethod {
        params: new_params,
        body: new_body,
        ..method
    }
}
