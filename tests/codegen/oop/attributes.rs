//! Purpose:
//! End-to-end codegen tests for PHP attribute syntax and the compile-time or
//! runtime behavior of supported built-in attributes.
//!
//! Called from:
//! - `cargo test` through Rust's test harness.
//!
//! Key details:
//! - Most user-defined attributes should not change output.
//! - Built-in attributes must respect PHP class-name resolution.

use super::*;

#[test]
fn test_attributes_do_not_alter_runtime_behavior() {
    // A class decorated with several attributes must compile to the same
    // observable behavior as the equivalent class without them.
    let out = compile_and_run(
        r#"<?php
#[Foo]
#[Bar(1, "two")]
class Counter {
    #[Slot]
    public int $n = 0;

    #[Mutator]
    public function inc(): void {
        $this->n = $this->n + 1;
    }
}

$c = new Counter();
$c->inc();
$c->inc();
$c->inc();
echo $c->n;
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn test_attribute_on_function_decl_compiles() {
    let out = compile_and_run(
        r#"<?php
#[Memoized]
function double(int $x): int {
    return $x * 2;
}
echo double(7);
"#,
    );
    assert_eq!(out, "14");
}

#[test]
fn test_qualified_attribute_name_compiles() {
    // Symfony-style attributes use fully-qualified names; the parser must
    // accept them and the codegen must emit unchanged output.
    let out = compile_and_run(
        r#"<?php
#[\App\Annotations\Mark]
class Tagged {
    #[\Symfony\Contracts\Service\Attribute\Required]
    public function setUp(): void {
    }
}

$t = new Tagged();
$t->setUp();
echo "ok";
"#,
    );
    assert_eq!(out, "ok");
}

#[test]
fn test_php_hash_line_comment_is_ignored() {
    let out = compile_and_run(
        r#"<?php
# this is a PHP-style line comment
echo 1;
# trailing comment with no newline at end"#,
    );
    assert_eq!(out, "1");
}

#[test]
fn test_parameter_attribute_compiles() {
    // Attributes on function parameters must compile identically to the
    // bare version.
    let out = compile_and_run(
        r#"<?php
function hash_password(#[Sensitive] string $pw): string {
    return $pw . "_hashed";
}
echo hash_password("secret");
"#,
    );
    assert_eq!(out, "secret_hashed");
}

#[test]
fn test_promoted_property_attribute_compiles() {
    let out = compile_and_run(
        r#"<?php
class Logger {
    public function __construct(#[Inject] public string $prefix) {}
}
$l = new Logger("[L] ");
echo $l->prefix;
"#,
    );
    assert_eq!(out, "[L] ");
}

#[test]
fn test_closure_attribute_compiles() {
    let out = compile_and_run(
        r#"<?php
$double = #[Pure] function (int $x): int { return $x * 2; };
echo $double(21);
"#,
    );
    assert_eq!(out, "42");
}

#[test]
fn test_arrow_function_attribute_compiles() {
    let out = compile_and_run(
        r#"<?php
$inc = #[Pure] fn (int $x) => $x + 1;
echo $inc(41);
"#,
    );
    assert_eq!(out, "42");
}

#[test]
fn test_static_closure_attribute_compiles() {
    let out = compile_and_run(
        r#"<?php
$triple = #[Pure] static function (int $x): int { return $x * 3; };
echo $triple(14);
"#,
    );
    assert_eq!(out, "42");
}

#[test]
fn test_closure_parameter_attribute_compiles() {
    let out = compile_and_run(
        r#"<?php
$mask = function (#[Sensitive] string $pw): string { return "***"; };
echo $mask("secret");
"#,
    );
    assert_eq!(out, "***");
}

#[test]
fn test_override_attribute_on_valid_override_compiles() {
    // Method does override a parent — `#[\Override]` should pass and behave
    // identically to the same method without the attribute.
    let out = compile_and_run(
        r#"<?php
class Animal {
    public function name(): string { return "Animal"; }
}
class Dog extends Animal {
    #[\Override]
    public function name(): string { return "Dog"; }
}
$d = new Dog();
echo $d->name();
"#,
    );
    assert_eq!(out, "Dog");
}

#[test]
fn test_override_attribute_through_interface_compiles() {
    // `#[\Override]` on an interface implementation must accept the inherited
    // signature.
    let out = compile_and_run(
        r#"<?php
interface Greeter {
    public function hello(): string;
}
class Hi implements Greeter {
    #[\Override]
    public function hello(): string { return "hi"; }
}
$g = new Hi();
echo $g->hello();
"#,
    );
    assert_eq!(out, "hi");
}

// --- #[\AllowDynamicProperties] runtime support (PHP 8.2) ---

#[test]
fn test_allow_dynamic_properties_basic_int() {
    let out = compile_and_run(
        r#"<?php
#[\AllowDynamicProperties]
class Bag {}
$b = new Bag();
$b->n = 42;
echo $b->n;
"#,
    );
    assert_eq!(out, "42");
}

#[test]
fn test_allow_dynamic_properties_string_value() {
    let out = compile_and_run(
        r#"<?php
#[\AllowDynamicProperties]
class Bag {}
$b = new Bag();
$b->msg = "hello";
echo $b->msg;
"#,
    );
    assert_eq!(out, "hello");
}

#[test]
fn test_allow_dynamic_properties_overwrite() {
    let out = compile_and_run(
        r#"<?php
#[\AllowDynamicProperties]
class Bag {}
$b = new Bag();
$b->v = 1;
$b->v = 2;
$b->v = 3;
echo $b->v;
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn test_allow_dynamic_properties_mixed_with_declared() {
    let out = compile_and_run(
        r#"<?php
#[\AllowDynamicProperties]
class Bag {
    public int $declared = 7;
}
$b = new Bag();
$b->extra = 13;
echo $b->declared;
echo "|";
echo $b->extra;
"#,
    );
    assert_eq!(out, "7|13");
}

#[test]
fn test_allow_dynamic_properties_unqualified_form() {
    let out = compile_and_run(
        r#"<?php
#[AllowDynamicProperties]
class Bag {}
$b = new Bag();
$b->x = 99;
echo $b->x;
"#,
    );
    assert_eq!(out, "99");
}

#[test]
fn test_allow_dynamic_properties_import_alias() {
    let out = compile_and_run(
        r#"<?php
use AllowDynamicProperties as DynamicBag;
#[DynamicBag]
class Bag {}
$b = new Bag();
$b->x = 55;
echo $b->x;
"#,
    );
    assert_eq!(out, "55");
}

#[test]
fn test_allow_dynamic_properties_is_inherited() {
    let out = compile_and_run(
        r#"<?php
#[\AllowDynamicProperties]
class Base {}
class Child extends Base {}
$c = new Child();
$c->x = 7;
echo $c->x;
"#,
    );
    assert_eq!(out, "7");
}

#[test]
fn test_allow_dynamic_properties_multiple_keys() {
    let out = compile_and_run(
        r#"<?php
#[\AllowDynamicProperties]
class Cfg {}
$c = new Cfg();
$c->host = "localhost";
$c->port = 8080;
$c->ssl = 1;
echo $c->host;
echo ":";
echo $c->port;
echo "/";
echo $c->ssl;
"#,
    );
    assert_eq!(out, "localhost:8080/1");
}
