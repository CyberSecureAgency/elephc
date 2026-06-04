//! Purpose:
//! Converts supported literal property defaults into backend-native values.
//! Keeps EIR object and static-property initialization on the same subset.
//!
//! Called from:
//! - `crate::codegen_ir::block_emit` static-property initialization.
//! - `crate::codegen_ir::lower_inst::objects` object allocation.
//!
//! Key details:
//! - This is intentionally narrower than full PHP expression lowering: only
//!   scalar and string literals that can be copied directly into storage land here.

use crate::parser::ast::ExprKind;
use crate::types::PhpType;

use super::{CodegenIrError, Result};

/// Literal default value that the EIR backend can write directly.
pub(crate) enum LiteralDefaultValue {
    Int(i64),
    Bool(bool),
    Float(f64),
    Str(String),
}

/// Converts a supported default expression into a direct storage value.
pub(crate) fn literal_default_value(
    context: &str,
    php_type: &PhpType,
    expr: &ExprKind,
    op_name: &str,
) -> Result<LiteralDefaultValue> {
    match (php_type, expr) {
        (PhpType::Int, ExprKind::IntLiteral(value)) => Ok(LiteralDefaultValue::Int(*value)),
        (PhpType::Int, ExprKind::Negate(inner)) => match &inner.kind {
            ExprKind::IntLiteral(value) => value
                .checked_neg()
                .map(LiteralDefaultValue::Int)
                .ok_or_else(|| unsupported_literal_default(context, php_type, op_name)),
            _ => Err(unsupported_literal_default(context, php_type, op_name)),
        },
        (PhpType::Bool, ExprKind::BoolLiteral(value)) => Ok(LiteralDefaultValue::Bool(*value)),
        (PhpType::Float, ExprKind::FloatLiteral(value)) => Ok(LiteralDefaultValue::Float(*value)),
        (PhpType::Float, ExprKind::IntLiteral(value)) => Ok(LiteralDefaultValue::Float(*value as f64)),
        (PhpType::Float, ExprKind::Negate(inner)) => match &inner.kind {
            ExprKind::FloatLiteral(value) => Ok(LiteralDefaultValue::Float(-value)),
            ExprKind::IntLiteral(value) => Ok(LiteralDefaultValue::Float(-(*value as f64))),
            _ => Err(unsupported_literal_default(context, php_type, op_name)),
        },
        (PhpType::Str, ExprKind::StringLiteral(value)) => Ok(LiteralDefaultValue::Str(value.clone())),
        _ => Err(unsupported_literal_default(context, php_type, op_name)),
    }
}

/// Builds the unsupported-feature error for default forms outside this slice.
fn unsupported_literal_default(
    context: &str,
    php_type: &PhpType,
    op_name: &str,
) -> CodegenIrError {
    CodegenIrError::unsupported(format!(
        "{} for default value of {} with PHP type {:?}",
        op_name, context, php_type
    ))
}
