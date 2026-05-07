use std::collections::HashMap;

use crate::errors::CompileError;
use crate::names::php_symbol_key;
use crate::parser::ast::{
    ClassMethod, ClassProperty, Expr, ExprKind, Stmt, StmtKind, TypeExpr, Visibility,
};
use crate::types::traits::FlattenedClass;
use crate::types::PhpType;

use super::Checker;

pub(crate) struct InterfaceDeclInfo {
    pub name: String,
    pub extends: Vec<String>,
    pub methods: Vec<crate::parser::ast::ClassMethod>,
    pub span: crate::span::Span,
}

impl Clone for InterfaceDeclInfo {
    fn clone(&self) -> Self {
        InterfaceDeclInfo {
            name: self.name.clone(),
            extends: self.extends.clone(),
            methods: self.methods.clone(),
            span: self.span,
        }
    }
}

pub(crate) fn inject_builtin_throwables(
    interface_map: &mut HashMap<String, InterfaceDeclInfo>,
    class_map: &mut HashMap<String, FlattenedClass>,
) -> Result<(), CompileError> {
    for builtin_name in ["Throwable", "Exception", "Fiber", "FiberError"] {
        let builtin_key = php_symbol_key(builtin_name);
        if interface_map
            .keys()
            .any(|name| php_symbol_key(name) == builtin_key)
            || class_map
                .keys()
                .any(|name| php_symbol_key(name) == builtin_key)
        {
            return Err(CompileError::new(
                crate::span::Span::dummy(),
                &format!("Cannot redeclare built-in type: {}", builtin_name),
            ));
        }
    }

    interface_map.insert(
        "Throwable".to_string(),
        InterfaceDeclInfo {
            name: "Throwable".to_string(),
            extends: Vec::new(),
            methods: vec![builtin_throwable_get_message_method()],
            span: crate::span::Span::dummy(),
        },
    );
    class_map.insert(
        "Exception".to_string(),
        FlattenedClass {
            name: "Exception".to_string(),
            extends: None,
            implements: vec!["Throwable".to_string()],
            is_abstract: false,
            is_final: false,
            is_readonly_class: false,
            properties: vec![builtin_exception_message_property()],
            methods: vec![
                builtin_exception_constructor_method(),
                builtin_exception_get_message_method(),
            ],
        },
    );

    // Fiber: cooperative coroutine class. Methods are placeholders here — the
    // codegen intercepts every Fiber operation (`new Fiber(...)`, instance
    // methods, `Fiber::suspend`, `Fiber::getCurrent`) and emits direct calls
    // into the `__rt_fiber_*` runtime helpers. Bodies are nominal returns so
    // the type checker sees a well-formed declaration.
    class_map.insert(
        "Fiber".to_string(),
        FlattenedClass {
            name: "Fiber".to_string(),
            extends: None,
            implements: Vec::new(),
            is_abstract: false,
            is_final: true,
            is_readonly_class: false,
            properties: Vec::new(),
            methods: builtin_fiber_methods(),
        },
    );

    // FiberError: extends the standard Exception so catch(Exception) and
    // catch(FiberError) both behave per PHP semantics.
    class_map.insert(
        "FiberError".to_string(),
        FlattenedClass {
            name: "FiberError".to_string(),
            extends: Some("Exception".to_string()),
            implements: Vec::new(),
            is_abstract: false,
            is_final: false,
            is_readonly_class: false,
            properties: Vec::new(),
            methods: Vec::new(),
        },
    );

    Ok(())
}

fn builtin_exception_message_property() -> ClassProperty {
    ClassProperty {
        name: "message".to_string(),
        visibility: Visibility::Public,
        type_expr: Some(TypeExpr::Str),
        readonly: false,
        is_final: false,
        is_static: false,
        by_ref: false,
        default: Some(Expr::new(
            ExprKind::StringLiteral(String::new()),
            crate::span::Span::dummy(),
        )),
        span: crate::span::Span::dummy(),
    }
}

fn builtin_exception_constructor_method() -> ClassMethod {
    ClassMethod {
        name: "__construct".to_string(),
        visibility: Visibility::Public,
        is_static: false,
        is_abstract: false,
        is_final: false,
        has_body: true,
        params: vec![(
            "message".to_string(),
            None,
            Some(Expr::new(
                ExprKind::StringLiteral(String::new()),
                crate::span::Span::dummy(),
            )),
            false,
        )],
        variadic: None,
        return_type: None,
        body: vec![Stmt::new(
            StmtKind::PropertyAssign {
                object: Box::new(Expr::new(ExprKind::This, crate::span::Span::dummy())),
                property: "message".to_string(),
                value: Expr::new(
                    ExprKind::Variable("message".to_string()),
                    crate::span::Span::dummy(),
                ),
            },
            crate::span::Span::dummy(),
        )],
        span: crate::span::Span::dummy(),
    }
}

fn builtin_exception_get_message_method() -> ClassMethod {
    ClassMethod {
        name: "getMessage".to_string(),
        visibility: Visibility::Public,
        is_static: false,
        is_abstract: false,
        is_final: false,
        has_body: true,
        params: Vec::new(),
        variadic: None,
        return_type: None,
        body: vec![Stmt::new(
            StmtKind::Return(Some(Expr::new(
                ExprKind::PropertyAccess {
                    object: Box::new(Expr::new(ExprKind::This, crate::span::Span::dummy())),
                    property: "message".to_string(),
                },
                crate::span::Span::dummy(),
            ))),
            crate::span::Span::dummy(),
        )],
        span: crate::span::Span::dummy(),
    }
}

fn fiber_method_dummy_body_return_null() -> Vec<Stmt> {
    vec![Stmt::new(
        StmtKind::Return(Some(Expr::new(
            ExprKind::Null,
            crate::span::Span::dummy(),
        ))),
        crate::span::Span::dummy(),
    )]
}

fn fiber_method_dummy_body_return_false() -> Vec<Stmt> {
    vec![Stmt::new(
        StmtKind::Return(Some(Expr::new(
            ExprKind::BoolLiteral(false),
            crate::span::Span::dummy(),
        ))),
        crate::span::Span::dummy(),
    )]
}

fn builtin_fiber_methods() -> Vec<ClassMethod> {
    let span = crate::span::Span::dummy();
    let null_default = || Some(Expr::new(ExprKind::Null, span));
    let is_state_predicate =
        |name: &str| ClassMethod {
            name: name.to_string(),
            visibility: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_final: true,
            has_body: true,
            params: Vec::new(),
            variadic: None,
            return_type: None,
            body: fiber_method_dummy_body_return_false(),
            span,
        };

    vec![
        // __construct(callable $callback): void
        ClassMethod {
            name: "__construct".to_string(),
            visibility: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_final: true,
            has_body: true,
            params: vec![("callback".to_string(), None, None, false)],
            variadic: None,
            return_type: None,
            body: Vec::new(),
            span,
        },
        // start(): mixed — bodies are dummy because codegen intercepts the call.
        // (Variadic ...$args support is deferred — closures captured by Fiber currently
        // receive no arguments at start time.)
        ClassMethod {
            name: "start".to_string(),
            visibility: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_final: true,
            has_body: true,
            params: Vec::new(),
            variadic: None,
            return_type: None,
            body: fiber_method_dummy_body_return_null(),
            span,
        },
        // resume(?$value = null): mixed
        ClassMethod {
            name: "resume".to_string(),
            visibility: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_final: true,
            has_body: true,
            params: vec![("value".to_string(), None, null_default(), false)],
            variadic: None,
            return_type: None,
            body: fiber_method_dummy_body_return_null(),
            span,
        },
        // throw(Throwable $exception): mixed
        ClassMethod {
            name: "throw".to_string(),
            visibility: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_final: true,
            has_body: true,
            params: vec![("exception".to_string(), None, None, false)],
            variadic: None,
            return_type: None,
            body: fiber_method_dummy_body_return_null(),
            span,
        },
        // getReturn(): mixed
        ClassMethod {
            name: "getReturn".to_string(),
            visibility: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_final: true,
            has_body: true,
            params: Vec::new(),
            variadic: None,
            return_type: None,
            body: fiber_method_dummy_body_return_null(),
            span,
        },
        // isStarted/isSuspended/isRunning/isTerminated(): bool
        is_state_predicate("isStarted"),
        is_state_predicate("isSuspended"),
        is_state_predicate("isRunning"),
        is_state_predicate("isTerminated"),
        // static suspend($value = null): mixed
        ClassMethod {
            name: "suspend".to_string(),
            visibility: Visibility::Public,
            is_static: true,
            is_abstract: false,
            is_final: true,
            has_body: true,
            params: vec![("value".to_string(), None, null_default(), false)],
            variadic: None,
            return_type: None,
            body: fiber_method_dummy_body_return_null(),
            span,
        },
        // static getCurrent(): ?Fiber
        ClassMethod {
            name: "getCurrent".to_string(),
            visibility: Visibility::Public,
            is_static: true,
            is_abstract: false,
            is_final: true,
            has_body: true,
            params: Vec::new(),
            variadic: None,
            return_type: None,
            body: fiber_method_dummy_body_return_null(),
            span,
        },
    ]
}

fn builtin_throwable_get_message_method() -> ClassMethod {
    ClassMethod {
        name: "getMessage".to_string(),
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

pub(crate) fn patch_builtin_exception_signatures(checker: &mut Checker) {
    if let Some(interface_info) = checker.interfaces.get_mut("Throwable") {
        if let Some(sig) = interface_info.methods.get_mut(&php_symbol_key("getMessage")) {
            sig.return_type = PhpType::Str;
        }
    }
    if let Some(class_info) = checker.classes.get_mut("Exception") {
        if let Some(sig) = class_info.methods.get_mut("__construct") {
            if let Some(param) = sig.params.get_mut(0) {
                param.1 = PhpType::Str;
            }
            sig.return_type = PhpType::Void;
        }
        if let Some(sig) = class_info.methods.get_mut(&php_symbol_key("getMessage")) {
            sig.return_type = PhpType::Str;
        }
    }
}

pub(crate) fn patch_builtin_fiber_signatures(checker: &mut Checker) {
    // Values transferred in/out of a fiber are typed `mixed` so the codegen
    // boxes scalars (int, string, …) into Mixed cells at the call site. The
    // runtime then just shuffles 8-byte cell pointers through transfer_value;
    // the type tag rides along inside the heap cell that the pointer addresses.
    let throwable_ty = PhpType::Object("Throwable".to_string());
    let Some(class_info) = checker.classes.get_mut("Fiber") else {
        return;
    };

    if let Some(sig) = class_info.methods.get_mut("__construct") {
        if let Some(param) = sig.params.get_mut(0) {
            param.1 = PhpType::Callable;
        }
        sig.return_type = PhpType::Void;
    }
    if let Some(sig) = class_info.methods.get_mut("start") {
        // Allow up to 7 Mixed arguments to be forwarded to the fiber's closure
        // — that exhausts the AArch64 integer arg registers available after
        // $this. Each slot has a `null` default so $f->start() with no args
        // still type-checks, while $f->start($a, $b) fills slots 0..2 and
        // leaves slots 2..7 at the null default. Closures with `use(...)`
        // captures are not yet supported — captures would be passed as
        // trailing arguments, which we cannot supply from here.
        let span = crate::span::Span::dummy();
        sig.params = (0..7)
            .map(|i| (format!("arg{}", i), PhpType::Mixed))
            .collect();
        sig.defaults = (0..7)
            .map(|_| Some(Expr::new(ExprKind::Null, span)))
            .collect();
        sig.ref_params = vec![false; 7];
        sig.declared_params = vec![false; 7];
        sig.return_type = PhpType::Mixed;
    }
    if let Some(sig) = class_info.methods.get_mut("resume") {
        if let Some(param) = sig.params.get_mut(0) {
            param.1 = PhpType::Mixed;
        }
        sig.return_type = PhpType::Mixed;
    }
    if let Some(sig) = class_info.methods.get_mut("throw") {
        if let Some(param) = sig.params.get_mut(0) {
            param.1 = throwable_ty.clone();
        }
        sig.return_type = PhpType::Mixed;
    }
    if let Some(sig) = class_info.methods.get_mut("getReturn") {
        sig.return_type = PhpType::Mixed;
    }
    for predicate in ["isStarted", "isSuspended", "isRunning", "isTerminated"] {
        if let Some(sig) = class_info.methods.get_mut(predicate) {
            sig.return_type = PhpType::Bool;
        }
    }
    if let Some(sig) = class_info.methods.get_mut("suspend") {
        if let Some(param) = sig.params.get_mut(0) {
            param.1 = PhpType::Mixed;
        }
        sig.return_type = PhpType::Mixed;
    }
    if let Some(sig) = class_info.methods.get_mut("getCurrent") {
        sig.return_type = PhpType::Mixed;
    }
}

pub(crate) fn patch_magic_method_signatures(checker: &mut Checker) {
    for class_info in checker.classes.values_mut() {
        if let Some(sig) = class_info.methods.get_mut("__get") {
            if let Some(param) = sig.params.get_mut(0) {
                param.1 = PhpType::Str;
            }
        }
        if let Some(sig) = class_info.methods.get_mut("__set") {
            if let Some(param) = sig.params.get_mut(0) {
                param.1 = PhpType::Str;
            }
            if let Some(param) = sig.params.get_mut(1) {
                param.1 = PhpType::Mixed;
            }
        }
    }
}

pub(crate) fn validate_magic_method_contracts(checker: &Checker) -> Result<(), CompileError> {
    let mut errors = Vec::new();
    for (class_name, class_info) in &checker.classes {
        for method in &class_info.method_decls {
            match php_symbol_key(&method.name).as_str() {
                "__tostring" => {
                    if method.is_static {
                        errors.push(CompileError::new(
                            method.span,
                            &format!(
                                "Magic method must be non-static: {}::__toString",
                                class_name
                            ),
                        ));
                        continue;
                    }
                    if method.visibility != Visibility::Public {
                        errors.push(CompileError::new(
                            method.span,
                            &format!("Magic method must be public: {}::__toString", class_name),
                        ));
                        continue;
                    }
                    if !method.params.is_empty() || method.variadic.is_some() {
                        errors.push(CompileError::new(
                            method.span,
                            &format!(
                                "Magic method must take 0 arguments: {}::__toString",
                                class_name
                            ),
                        ));
                        continue;
                    }
                    if class_info
                        .methods
                        .get("__tostring")
                        .map(|sig| sig.return_type.clone())
                        != Some(PhpType::Str)
                    {
                        errors.push(CompileError::new(
                            method.span,
                            &format!(
                                "Magic method must return string: {}::__toString",
                                class_name
                            ),
                        ));
                    }
                }
                "__get" => {
                    if method.is_static {
                        errors.push(CompileError::new(
                            method.span,
                            &format!("Magic method must be non-static: {}::__get", class_name),
                        ));
                        continue;
                    }
                    if method.visibility != Visibility::Public {
                        errors.push(CompileError::new(
                            method.span,
                            &format!("Magic method must be public: {}::__get", class_name),
                        ));
                        continue;
                    }
                    if method.params.len() != 1 || method.variadic.is_some() {
                        errors.push(CompileError::new(
                            method.span,
                            &format!("Magic method must take 1 argument: {}::__get", class_name),
                        ));
                    }
                }
                "__set" => {
                    if method.is_static {
                        errors.push(CompileError::new(
                            method.span,
                            &format!("Magic method must be non-static: {}::__set", class_name),
                        ));
                        continue;
                    }
                    if method.visibility != Visibility::Public {
                        errors.push(CompileError::new(
                            method.span,
                            &format!("Magic method must be public: {}::__set", class_name),
                        ));
                        continue;
                    }
                    if method.params.len() != 2 || method.variadic.is_some() {
                        errors.push(CompileError::new(
                            method.span,
                            &format!("Magic method must take 2 arguments: {}::__set", class_name),
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CompileError::from_many(errors))
    }
}
