//! Purpose:
//! Defines the reusable AST walker contract for magic-constant substitution passes.
//! Provides pass hooks for magic constants and scope entry/exit events.
//!
//! Called from:
//! - `crate::magic_constants::file_pass`, `scope_pass`, and `trait_binding`.
//!
//! Key details:
//! - Walkers rebuild AST nodes while preserving spans and delegating context-specific substitutions to `Pass`.

mod exprs;
mod members;
mod stmts;

use crate::names::Name;
use crate::parser::ast::{ExprKind, MagicConstant};
use crate::span::Span;

pub(super) use members::{walk_class_method, walk_class_property};
pub(super) use stmts::walk_program;

pub(super) trait Pass {
    fn transform_magic(&self, span: Span, mc: MagicConstant) -> ExprKind;
    fn transform_string(&self, value: String) -> ExprKind {
        ExprKind::StringLiteral(value)
    }

    fn enter_namespace_decl(&mut self, _name: &Option<Name>) {}
    fn enter_namespace_block(&mut self, _name: &Option<Name>) {}
    fn leave_namespace_block(&mut self) {}
    fn enter_function(&mut self, _name: &str) {}
    fn leave_function(&mut self) {}
    fn enter_class(&mut self, _name: &str) {}
    fn leave_class(&mut self) {}
    fn enter_trait(&mut self, _name: &str) {}
    fn leave_trait(&mut self) {}
    fn enter_method(&mut self, _name: &str) {}
    fn leave_method(&mut self) {}
    fn enter_closure(&mut self, _span: Span) {}
    fn leave_closure(&mut self) {}
}
