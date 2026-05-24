//! Purpose:
//! Injects SPL container class metadata into the checker.
//! Provides nominal class/interface/signature contracts for runtime-backed SPL containers.
//!
//! Called from:
//! - `crate::types::checker::driver`
//!
//! Key details:
//! - Direct storage and legacy serialization methods use runtime `IntrinsicCall` backing.
//! - Structured serialization/debug helpers keep small synthetic PHP bodies.

use std::collections::HashMap;

use crate::errors::CompileError;
use crate::names::{php_symbol_key, Name};
use crate::parser::ast::{
    BinOp, ClassConst, ClassMethod, Expr, ExprKind, Stmt, StmtKind, TypeExpr, Visibility,
};
use crate::types::traits::FlattenedClass;

use super::builtin_types::InterfaceDeclInfo;

/// Injects SplDoublyLinkedList, SplStack, SplQueue, and SplFixedArray into the
/// checker's class_map with full method signatures and interface contracts.
/// Fails if any SPL class name is already registered in interface_map or class_map.
pub(crate) fn inject_builtin_spl_classes(
    interface_map: &mut HashMap<String, InterfaceDeclInfo>,
    class_map: &mut HashMap<String, FlattenedClass>,
) -> Result<(), CompileError> {
    for class_name in PHASE4_SPL_CLASS_NAMES {
        let class_key = php_symbol_key(class_name);
        if interface_map
            .keys()
            .any(|name| php_symbol_key(name) == class_key)
            || class_map
                .keys()
                .any(|name| php_symbol_key(name) == class_key)
        {
            return Err(CompileError::new(
                crate::span::Span::dummy(),
                &format!("Cannot redeclare built-in SPL class: {}", class_name),
            ));
        }
    }

    class_map.insert(
        "SplDoublyLinkedList".to_string(),
        FlattenedClass {
            name: "SplDoublyLinkedList".to_string(),
            extends: None,
            implements: vec![
                "Iterator".to_string(),
                "Countable".to_string(),
                "ArrayAccess".to_string(),
            ],
            is_abstract: false,
            is_final: false,
            is_readonly_class: false,
            properties: Vec::new(),
            methods: spl_doubly_linked_list_methods(),
            attributes: Vec::new(),
            constants: spl_doubly_linked_list_constants(),
        },
    );

    class_map.insert(
        "SplStack".to_string(),
        FlattenedClass {
            name: "SplStack".to_string(),
            extends: Some("SplDoublyLinkedList".to_string()),
            implements: Vec::new(),
            is_abstract: false,
            is_final: false,
            is_readonly_class: false,
            properties: Vec::new(),
            methods: Vec::new(),
            attributes: Vec::new(),
            constants: Vec::new(),
        },
    );

    class_map.insert(
        "SplQueue".to_string(),
        FlattenedClass {
            name: "SplQueue".to_string(),
            extends: Some("SplDoublyLinkedList".to_string()),
            implements: Vec::new(),
            is_abstract: false,
            is_final: false,
            is_readonly_class: false,
            properties: Vec::new(),
            methods: vec![
                method("enqueue", vec![param("value", mixed_type())], Some(TypeExpr::Void)),
                method("dequeue", Vec::new(), Some(mixed_type())),
            ],
            attributes: Vec::new(),
            constants: Vec::new(),
        },
    );

    class_map.insert(
        "SplFixedArray".to_string(),
        FlattenedClass {
            name: "SplFixedArray".to_string(),
            extends: None,
            implements: vec![
                "ArrayAccess".to_string(),
                "Countable".to_string(),
                "JsonSerializable".to_string(),
            ],
            is_abstract: false,
            is_final: false,
            is_readonly_class: false,
            properties: Vec::new(),
            methods: spl_fixed_array_methods(),
            attributes: Vec::new(),
            constants: Vec::new(),
        },
    );

    Ok(())
}

const PHASE4_SPL_CLASS_NAMES: &[&str] = &[
    "SplDoublyLinkedList",
    "SplStack",
    "SplQueue",
    "SplFixedArray",
];

/// Returns the method definitions for SplDoublyLinkedList, including Iterator,
/// Countable, ArrayAccess, and Serializable/JSONSerializable interface methods.
fn spl_doubly_linked_list_methods() -> Vec<ClassMethod> {
    vec![
        method(
            "add",
            vec![param("index", TypeExpr::Int), param("value", mixed_type())],
            Some(TypeExpr::Void),
        ),
        method("pop", Vec::new(), Some(mixed_type())),
        method("shift", Vec::new(), Some(mixed_type())),
        method("push", vec![param("value", mixed_type())], Some(TypeExpr::Void)),
        method(
            "unshift",
            vec![param("value", mixed_type())],
            Some(TypeExpr::Void),
        ),
        method("top", Vec::new(), Some(mixed_type())),
        method("bottom", Vec::new(), Some(mixed_type())),
        method("count", Vec::new(), Some(TypeExpr::Int)),
        method("isEmpty", Vec::new(), Some(TypeExpr::Bool)),
        method(
            "setIteratorMode",
            vec![param("mode", TypeExpr::Int)],
            Some(TypeExpr::Void),
        ),
        method("getIteratorMode", Vec::new(), Some(TypeExpr::Int)),
        method(
            "offsetExists",
            vec![param("index", mixed_type())],
            Some(TypeExpr::Bool),
        ),
        method(
            "offsetGet",
            vec![param("index", mixed_type())],
            Some(mixed_type()),
        ),
        method(
            "offsetSet",
            vec![param("index", mixed_type()), param("value", mixed_type())],
            Some(TypeExpr::Void),
        ),
        method(
            "offsetUnset",
            vec![param("index", mixed_type())],
            Some(TypeExpr::Void),
        ),
        method("rewind", Vec::new(), Some(TypeExpr::Void)),
        method("current", Vec::new(), Some(mixed_type())),
        method("key", Vec::new(), Some(mixed_type())),
        method("prev", Vec::new(), Some(TypeExpr::Void)),
        method("next", Vec::new(), Some(TypeExpr::Void)),
        method("valid", Vec::new(), Some(TypeExpr::Bool)),
        method("serialize", Vec::new(), Some(TypeExpr::Str)),
        method(
            "unserialize",
            vec![param("data", TypeExpr::Str)],
            Some(TypeExpr::Void),
        ),
        method_with_body(
            "__serialize",
            Vec::new(),
            Some(array_type()),
            dll_serialize_array_body(),
        ),
        method_with_body(
            "__unserialize",
            vec![param("data", array_type())],
            Some(TypeExpr::Void),
            dll_unserialize_body(),
        ),
        method_with_body(
            "__debugInfo",
            Vec::new(),
            Some(array_type()),
            dll_debug_info_body(),
        ),
    ]
}

/// Returns the method definitions for SplFixedArray, including ArrayAccess,
/// Countable, JsonSerializable, and factory-method fromArray.
fn spl_fixed_array_methods() -> Vec<ClassMethod> {
    vec![
        method(
            "__construct",
            vec![param_default("size", TypeExpr::Int, int_expr(0))],
            Some(TypeExpr::Void),
        ),
        method_with_body("__wakeup", Vec::new(), Some(TypeExpr::Void), Vec::new()),
        class_method(
            "fromArray",
            true,
            vec![
                param("array", array_type()),
                param_default("preserveKeys", TypeExpr::Bool, bool_expr(true)),
            ],
            Some(named_type("SplFixedArray")),
        ),
        method_with_body(
            "__serialize",
            Vec::new(),
            Some(array_type()),
            vec![return_stmt(method_call(this_expr(), "toArray", Vec::new()))],
        ),
        method("__unserialize", vec![param("data", array_type())], Some(TypeExpr::Void)),
        method("count", Vec::new(), Some(TypeExpr::Int)),
        method("toArray", Vec::new(), Some(array_type())),
        method("getSize", Vec::new(), Some(TypeExpr::Int)),
        method(
            "setSize",
            vec![param("size", TypeExpr::Int)],
            Some(TypeExpr::Void),
        ),
        method(
            "offsetExists",
            vec![param("index", mixed_type())],
            Some(TypeExpr::Bool),
        ),
        method(
            "offsetGet",
            vec![param("index", mixed_type())],
            Some(mixed_type()),
        ),
        method(
            "offsetSet",
            vec![param("index", mixed_type()), param("value", mixed_type())],
            Some(TypeExpr::Void),
        ),
        method(
            "offsetUnset",
            vec![param("index", mixed_type())],
            Some(TypeExpr::Void),
        ),
        method("jsonSerialize", Vec::new(), Some(array_type())),
    ]
}

/// Returns the constant definitions for SplDoublyLinkedList (IT_MODE_* values).
fn spl_doubly_linked_list_constants() -> Vec<ClassConst> {
    vec![
        class_const("IT_MODE_LIFO", 2),
        class_const("IT_MODE_FIFO", 0),
        class_const("IT_MODE_DELETE", 1),
        class_const("IT_MODE_KEEP", 0),
    ]
}

/// Builds a non-static ClassMethod from name, params, and return type.
/// Params is a tuple of (name, type, default_expr, is_variadic).
fn method(
    name: &str,
    params: Vec<(String, Option<TypeExpr>, Option<Expr>, bool)>,
    return_type: Option<TypeExpr>,
) -> ClassMethod {
    class_method(name, false, params, return_type)
}

/// Builds a non-static ClassMethod with an explicit body of statements.
fn method_with_body(
    name: &str,
    params: Vec<(String, Option<TypeExpr>, Option<Expr>, bool)>,
    return_type: Option<TypeExpr>,
    body: Vec<Stmt>,
) -> ClassMethod {
    class_method_with_body(name, false, params, return_type, body)
}

/// Builds a ClassMethod (static or non-static) with a dummy body that returns
/// a zero-valued expression for the given return type.
fn class_method(
    name: &str,
    is_static: bool,
    params: Vec<(String, Option<TypeExpr>, Option<Expr>, bool)>,
    return_type: Option<TypeExpr>,
) -> ClassMethod {
    class_method_with_body(
        name,
        is_static,
        params,
        return_type.clone(),
        dummy_body_for(return_type.as_ref()),
    )
}

/// Builds a ClassMethod (static or non-static) with an explicit body of statements.
fn class_method_with_body(
    name: &str,
    is_static: bool,
    params: Vec<(String, Option<TypeExpr>, Option<Expr>, bool)>,
    return_type: Option<TypeExpr>,
    body: Vec<Stmt>,
) -> ClassMethod {
    ClassMethod {
        name: name.to_string(),
        visibility: Visibility::Public,
        is_static,
        is_abstract: false,
        is_final: false,
        has_body: true,
        params,
        variadic: None,
        return_type,
        body,
        span: crate::span::Span::dummy(),
        attributes: Vec::new(),
    }
}

/// Generates a dummy method body that returns a zero/empty value appropriate
/// for the return type: nothing for void, 0/false/" "/[] for scalar types, null otherwise.
fn dummy_body_for(return_type: Option<&TypeExpr>) -> Vec<Stmt> {
    match return_type {
        Some(TypeExpr::Void) | None => Vec::new(),
        Some(TypeExpr::Int) => return_body(int_expr(0)),
        Some(TypeExpr::Bool) => return_body(bool_expr(false)),
        Some(TypeExpr::Str) => return_body(Expr::new(
            ExprKind::StringLiteral(String::new()),
            crate::span::Span::dummy(),
        )),
        Some(TypeExpr::Named(name)) if name.as_canonical() == "array" => {
            return_body(Expr::new(ExprKind::ArrayLiteral(Vec::new()), crate::span::Span::dummy()))
        }
        _ => return_body(Expr::new(ExprKind::Null, crate::span::Span::dummy())),
    }
}

/// Wraps a value expression in a Return statement.
fn return_body(value: Expr) -> Vec<Stmt> {
    vec![return_stmt(value)]
}

/// Builds a Return statement wrapping the given expression.
fn return_stmt(value: Expr) -> Stmt {
    Stmt::new(StmtKind::Return(Some(value)), crate::span::Span::dummy())
}

/// Builds a parameter tuple with no default: (name, Some(type), None, false).
fn param(name: &str, ty: TypeExpr) -> (String, Option<TypeExpr>, Option<Expr>, bool) {
    (name.to_string(), Some(ty), None, false)
}

/// Builds a parameter tuple with a default expression: (name, Some(type), Some(default), false).
fn param_default(
    name: &str,
    ty: TypeExpr,
    default: Expr,
) -> (String, Option<TypeExpr>, Option<Expr>, bool) {
    (name.to_string(), Some(ty), Some(default), false)
}

/// Builds a public ClassConst with an integer value derived from int_expr(value).
fn class_const(name: &str, value: i64) -> ClassConst {
    ClassConst {
        name: name.to_string(),
        visibility: Visibility::Public,
        is_final: false,
        value: int_expr(value),
        span: crate::span::Span::dummy(),
        attributes: Vec::new(),
    }
}

/// Constructs a literal integer expression with the given i64 value.
fn int_expr(value: i64) -> Expr {
    Expr::new(ExprKind::IntLiteral(value), crate::span::Span::dummy())
}

/// Constructs a literal boolean expression from a bool value.
fn bool_expr(value: bool) -> Expr {
    Expr::new(ExprKind::BoolLiteral(value), crate::span::Span::dummy())
}

/// Returns a TypeExpr for the built-in `mixed` type.
fn mixed_type() -> TypeExpr {
    named_type("mixed")
}

/// Returns a TypeExpr for the built-in `array` type.
fn array_type() -> TypeExpr {
    named_type("array")
}

/// Returns a TypeExpr for an unqualified (global) type name.
fn named_type(name: &str) -> TypeExpr {
    TypeExpr::Named(Name::unqualified(name))
}

/// Wraps ExprKind in an Expr with a dummy span.
fn expr(kind: ExprKind) -> Expr {
    Expr::new(kind, crate::span::Span::dummy())
}

/// Constructs a string literal expression with the given static string value.
fn string_expr(value: &str) -> Expr {
    expr(ExprKind::StringLiteral(value.to_string()))
}

/// Constructs a variable reference expression for the given variable name.
fn var_expr(name: &str) -> Expr {
    expr(ExprKind::Variable(name.to_string()))
}

/// Constructs a `$this` expression.
fn this_expr() -> Expr {
    expr(ExprKind::This)
}

/// Constructs a binary operation expression from left, operator, and right.
fn binary_expr(left: Expr, op: BinOp, right: Expr) -> Expr {
    expr(ExprKind::BinaryOp {
        left: Box::new(left),
        op,
        right: Box::new(right),
    })
}

/// Constructs a logical NOT expression wrapping the given value.
fn not_expr(value: Expr) -> Expr {
    expr(ExprKind::Not(Box::new(value)))
}

/// Constructs a method-call expression on a given object with positional args.
fn method_call(object: Expr, method: &str, args: Vec<Expr>) -> Expr {
    expr(ExprKind::MethodCall {
        object: Box::new(object),
        method: method.to_string(),
        args,
    })
}

/// Constructs an array-access expression (array[index]).
fn array_access(array: Expr, index: Expr) -> Expr {
    expr(ExprKind::ArrayAccess {
        array: Box::new(array),
        index: Box::new(index),
    })
}

/// Constructs a simple assignment statement (name = value).
fn assign_stmt(name: &str, value: Expr) -> Stmt {
    Stmt::new(
        StmtKind::Assign {
            name: name.to_string(),
            value,
        },
        crate::span::Span::dummy(),
    )
}

/// Constructs a bare expression statement from the given expression.
fn expr_stmt(value: Expr) -> Stmt {
    Stmt::new(StmtKind::ExprStmt(value), crate::span::Span::dummy())
}

/// Constructs an array_push statement (array[] = value) for the named array.
fn array_push_stmt(array: &str, value: Expr) -> Stmt {
    Stmt::new(
        StmtKind::ArrayPush {
            array: array.to_string(),
            value,
        },
        crate::span::Span::dummy(),
    )
}

/// Constructs a while loop statement with the given condition and body statements.
fn while_stmt(condition: Expr, body: Vec<Stmt>) -> Stmt {
    Stmt::new(
        StmtKind::While { condition, body },
        crate::span::Span::dummy(),
    )
}

/// Constructs a foreach statement over an array, optionally binding the key to key_var.
/// value_by_ref controls whether the value is captured by reference.
fn foreach_stmt(array: Expr, key_var: Option<&str>, value_var: &str, body: Vec<Stmt>) -> Stmt {
    Stmt::new(
        StmtKind::Foreach {
            array,
            key_var: key_var.map(str::to_string),
            value_var: value_var.to_string(),
            value_by_ref: false,
            body,
        },
        crate::span::Span::dummy(),
    )
}

/// Constructs an assignment statement that increments the named variable by 1.
fn increment_stmt(name: &str) -> Stmt {
    assign_stmt(name, binary_expr(var_expr(name), BinOp::Add, int_expr(1)))
}

/// Builds the shared prelude for serializing a DLL: initializes `$items = []`,
/// `$i = 0`, `$limit = $this->count()`, then appends `$items[] = $this->offsetGet($i++)`
/// in a while loop until `$i < $limit`. Used by both serialize and debugInfo bodies.
fn dll_items_snapshot_prelude() -> Vec<Stmt> {
    vec![
        assign_stmt("items", expr(ExprKind::ArrayLiteral(Vec::new()))),
        assign_stmt("i", int_expr(0)),
        assign_stmt("limit", method_call(this_expr(), "count", Vec::new())),
        while_stmt(
            binary_expr(var_expr("i"), BinOp::Lt, var_expr("limit")),
            vec![
                array_push_stmt("items", method_call(this_expr(), "offsetGet", vec![var_expr("i")])),
                increment_stmt("i"),
            ],
        ),
    ]
}

/// Builds the body for SplDoublyLinkedList::__serialize: captures iterator mode
/// and a snapshot of all items as an array [mode, items, []].
fn dll_serialize_array_body() -> Vec<Stmt> {
    let mut body = dll_items_snapshot_prelude();
    body.push(return_stmt(expr(ExprKind::ArrayLiteral(vec![
        method_call(this_expr(), "getIteratorMode", Vec::new()),
        var_expr("items"),
        expr(ExprKind::ArrayLiteral(Vec::new())),
    ]))));
    body
}

/// Builds the body for SplDoublyLinkedList::__debugInfo: saves iterator mode,
/// switches to mode 0 (fifo, keep), snapshots items, restores original mode,
/// returns associative array [\0SplDoublyLinkedList\0flags => mode, \0SplDoublyLinkedList\0dllist => items].
fn dll_debug_info_body() -> Vec<Stmt> {
    let mut body = vec![
        assign_stmt("mode", method_call(this_expr(), "getIteratorMode", Vec::new())),
        expr_stmt(method_call(this_expr(), "setIteratorMode", vec![int_expr(0)])),
    ];
    body.extend(dll_items_snapshot_prelude());
    body.push(expr_stmt(method_call(
        this_expr(),
        "setIteratorMode",
        vec![var_expr("mode")],
    )));
    body.push(return_stmt(expr(ExprKind::ArrayLiteralAssoc(vec![
        (
            string_expr("\0SplDoublyLinkedList\0flags"),
            var_expr("mode"),
        ),
        (
            string_expr("\0SplDoublyLinkedList\0dllist"),
            var_expr("items"),
        ),
    ]))));
    body
}

/// Builds the body for SplDoublyLinkedList::__unserialize: sets iterator mode
/// from data[0], clears the list via pop in a while-not-empty loop, then pushes
/// all items from data[1] in order.
fn dll_unserialize_body() -> Vec<Stmt> {
    vec![
        expr_stmt(method_call(
            this_expr(),
            "setIteratorMode",
            vec![array_access(var_expr("data"), int_expr(0))],
        )),
        while_stmt(
            not_expr(method_call(this_expr(), "isEmpty", Vec::new())),
            vec![expr_stmt(method_call(this_expr(), "pop", Vec::new()))],
        ),
        foreach_stmt(
            array_access(var_expr("data"), int_expr(1)),
            None,
            "value",
            vec![expr_stmt(method_call(this_expr(), "push", vec![var_expr("value")]))],
        ),
    ]
}
