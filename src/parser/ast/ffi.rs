//! Purpose:
//! Defines AST records for elephc extern declarations and packed data layouts.
//! Represents C-facing scalar, pointer, buffer, function, global, and struct field metadata.
//!
//! Called from:
//! - `crate::parser::stmt::ffi` and downstream type/codegen FFI handling.
//!
//! Key details:
//! - These nodes describe compiler extensions, not PHP syntax, and must stay explicit in the AST.

use crate::span::Span;

use super::TypeExpr;

// --- FFI ---

/// C type annotation for extern declarations
#[derive(Debug, Clone, PartialEq)]
pub enum CType {
    Int,
    Float,
    Str,        // char* (null-terminated)
    Bool,
    Void,
    Ptr,                    // opaque void*
    TypedPtr(String),       // ptr<ClassName>
    Callable,               // function pointer
}

/// Parameter in an extern function declaration
#[derive(Debug, Clone, PartialEq)]
pub struct ExternParam {
    pub name: String,
    pub c_type: CType,
}

/// Field in an extern class (C struct) declaration
#[derive(Debug, Clone, PartialEq)]
pub struct ExternField {
    pub name: String,
    pub c_type: CType,
}

#[derive(Debug, Clone)]
pub struct PackedField {
    pub name: String,
    pub type_expr: TypeExpr,
    pub span: Span,
}

impl PartialEq for PackedField {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.type_expr == other.type_expr
    }
}
