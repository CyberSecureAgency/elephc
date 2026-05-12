//! Purpose:
//! `yield from` delegation: int-array-literal expansion, runtime delegation through an inner Generator value (function call result, local variable, argument passthrough).
//!
//! Called from:
//!  - `cargo test` via the integration test harness; aggregated under
//!    `tests::codegen::generators` in `tests/codegen/generators/mod.rs`.

use crate::support::*;

#[test]
fn test_generator_yield_from_int_array_literal() {
    // `yield from <int_array_literal>` desugars to one Yield node per
    // element at compile time, each carrying its own state index.
    let out = compile_and_run(
        r#"<?php
function delegate() {
    yield 0;
    yield from [10, 20, 30];
    yield 99;
}
foreach (delegate() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 10 20 30 99 ");
}

#[test]
fn test_generator_yield_from_local_generator_variable() {
    // `yield from $local` where the local holds a Generator pointer
    // (returned from another generator function call).
    let out = compile_and_run(
        r#"<?php
function inner() { yield 1; yield 2; yield 3; }
function outer() {
    $g = inner();
    yield from $g;
}
foreach (outer() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "1 2 3 ");
}

#[test]
fn test_generator_yield_from_inner_generator() {
    // Runtime delegation via the GeneratorFrame's `delegated_iter` slot:
    // outer yields 0, hands off to inner which yields 1/2/3, then yields
    // 99 once inner is exhausted.
    let out = compile_and_run(
        r#"<?php
function inner() { yield 1; yield 2; yield 3; }
function outer() {
    yield 0;
    yield from inner();
    yield 99;
}
foreach (outer() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 1 2 3 99 ");
}

#[test]
fn test_generator_yield_from_case_insensitive_from_keyword() {
    let out = compile_and_run(
        r#"<?php
function inner() { yield 1; yield 2; }
function outer() {
    yield 0;
    yield FROM inner();
}
foreach (outer() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 1 2 ");
}

#[test]
fn test_generator_yield_from_with_arg_passing() {
    let out = compile_and_run(
        r#"<?php
function range_gen(int $start, int $end) {
    $i = $start;
    while ($i < $end) {
        yield $i;
        $i++;
    }
}
function combined() {
    yield from range_gen(0, 3);
    yield from range_gen(10, 12);
}
foreach (combined() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 1 2 10 11 ");
}
