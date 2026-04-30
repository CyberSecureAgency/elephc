use std::collections::HashMap;

use crate::errors::CompileError;
use crate::parser::ast::{ClassMethod, Visibility};
use crate::types::traits::FlattenedClass;
use crate::types::PhpType;

use super::Checker;
use super::builtin_types::InterfaceDeclInfo;

pub(crate) fn inject_builtin_iterators(
    interface_map: &mut HashMap<String, InterfaceDeclInfo>,
    class_map: &mut HashMap<String, FlattenedClass>,
) -> Result<(), CompileError> {
    for builtin_name in ["Iterator", "IteratorAggregate"] {
        if interface_map.contains_key(builtin_name) || class_map.contains_key(builtin_name) {
            return Err(CompileError::new(
                crate::span::Span::dummy(),
                &format!("Cannot redeclare built-in interface: {}", builtin_name),
            ));
        }
    }

    interface_map.insert(
        "Iterator".to_string(),
        InterfaceDeclInfo {
            name: "Iterator".to_string(),
            extends: Vec::new(),
            methods: vec![
                builtin_iterator_method("current"),
                builtin_iterator_method("key"),
                builtin_iterator_method("next"),
                builtin_iterator_method("valid"),
                builtin_iterator_method("rewind"),
            ],
            span: crate::span::Span::dummy(),
        },
    );

    interface_map.insert(
        "IteratorAggregate".to_string(),
        InterfaceDeclInfo {
            name: "IteratorAggregate".to_string(),
            extends: Vec::new(),
            methods: vec![builtin_iterator_method("getIterator")],
            span: crate::span::Span::dummy(),
        },
    );

    Ok(())
}

fn builtin_iterator_method(name: &str) -> ClassMethod {
    ClassMethod {
        name: name.to_string(),
        visibility: Visibility::Public,
        is_static: false,
        is_abstract: true,
        is_final: false,
        has_body: false,
        params: Vec::new(),
        variadic: None,
        return_type: None,
        body: Vec::new(),
        span: crate::span::Span::dummy(),
    }
}

pub(crate) fn patch_builtin_iterator_signatures(checker: &mut Checker) {
    if let Some(info) = checker.interfaces.get_mut("Iterator") {
        for (name, ty) in &[
            ("current", PhpType::Mixed),
            ("key", PhpType::Mixed),
            ("next", PhpType::Void),
            ("valid", PhpType::Bool),
            ("rewind", PhpType::Void),
        ] {
            if let Some(sig) = info.methods.get_mut(*name) {
                sig.return_type = ty.clone();
            }
        }
    }
    if let Some(info) = checker.interfaces.get_mut("IteratorAggregate") {
        if let Some(sig) = info.methods.get_mut("getIterator") {
            // PHP returns Traversable; elephc treats it as Iterator since
            // we don't model Traversable as a separate parent interface.
            sig.return_type = PhpType::Object("Iterator".to_string());
        }
    }
}
