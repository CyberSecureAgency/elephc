use crate::parser::ast::Expr;

use super::PhpType;

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionSig {
    pub params: Vec<(String, PhpType)>,
    pub defaults: Vec<Option<Expr>>,
    pub return_type: PhpType,
    pub declared_return: bool,
    pub ref_params: Vec<bool>,
    pub declared_params: Vec<bool>,
    pub variadic: Option<String>,
}

pub(crate) fn first_class_callable_builtin_sig(name: &str) -> Option<FunctionSig> {
    match name {
        "strlen" => Some(FunctionSig {
            params: vec![("arg0".to_string(), PhpType::Str)],
            defaults: vec![None],
            return_type: PhpType::Int,
            declared_return: true,
            ref_params: vec![false],
            declared_params: vec![true],
            variadic: None,
        }),
        "count" => Some(FunctionSig {
            params: vec![(
                "arg0".to_string(),
                PhpType::AssocArray {
                    key: Box::new(PhpType::Mixed),
                    value: Box::new(PhpType::Mixed),
                },
            )],
            defaults: vec![None],
            return_type: PhpType::Int,
            declared_return: true,
            ref_params: vec![false],
            declared_params: vec![true],
            variadic: None,
        }),
        "buffer_len" => Some(FunctionSig {
            params: vec![("arg0".to_string(), PhpType::Buffer(Box::new(PhpType::Int)))],
            defaults: vec![None],
            return_type: PhpType::Int,
            declared_return: true,
            ref_params: vec![false],
            declared_params: vec![true],
            variadic: None,
        }),
        _ => None,
    }
}
