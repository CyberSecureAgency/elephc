use crate::support::*;

#[test]
fn test_generator_function_returns_generator_instance() {
    // The result of a generator function call is a real Generator object —
    // it satisfies `instanceof Generator` and `instanceof Iterator`.
    let out = compile_and_run(
        r#"<?php
function gen() {
    yield 1;
}
$g = gen();
if ($g instanceof Generator) { echo "G "; }
if ($g instanceof Iterator) { echo "I "; }
echo "done";
"#,
    );
    assert_eq!(out, "G I done");
}

#[test]
fn test_generator_method_calls_step_through_state() {
    // Stepping the generator manually: rewind() runs to the first yield,
    // valid() reports a value is available, current() returns it,
    // next() advances; after the last yield, valid() reports false.
    let out = compile_and_run(
        r#"<?php
function gen() { yield 7; yield 9; }
$g = gen();
$g->rewind();
echo $g->valid() ? "T" : "F";
echo $g->current();
$g->next();
echo $g->valid() ? "T" : "F";
echo $g->current();
$g->next();
echo $g->valid() ? "T" : "F";
"#,
    );
    assert_eq!(out, "T7T9F");
}

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
fn test_generator_int_division_in_yield_expr() {
    // `$i / 2 * 2 == $i` is true exactly when $i is even (signed integer
    // division truncates toward zero). The generator emits even numbers.
    let out = compile_and_run(
        r#"<?php
function gen(int $n) {
    for ($i = 0; $i < $n; $i++) {
        if ($i == $i / 2 * 2) {
            yield $i;
        }
    }
}
foreach (gen(10) as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 2 4 6 8 ");
}

#[test]
fn test_generator_yields_with_string_keys_and_int_values() {
    let out = compile_and_run(
        r#"<?php
function pairs() {
    yield "a" => 1;
    yield "b" => 2;
}
foreach (pairs() as $k => $v) {
    echo $k;
    echo $v;
}
"#,
    );
    assert_eq!(out, "a1b2");
}

#[test]
fn test_generator_switch_with_default_branch() {
    let out = compile_and_run(
        r#"<?php
function gen(int $n) {
    switch ($n) {
        case 1:
            yield "one";
            break;
        case 2:
            yield "two";
            break;
        default:
            yield "other";
    }
    yield $n;
}
foreach (gen(2) as $v) { echo $v; echo " "; }
echo "| ";
foreach (gen(7) as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "two 2 | other 7 ");
}

#[test]
fn test_generator_calls_user_function() {
    // `yield helper($i)` evaluates the user function call into x0 then
    // boxes the result. v1 supports up to 8 int arguments.
    let out = compile_and_run(
        r#"<?php
function helper(int $x): int { return $x + 100; }
function gen() {
    yield helper(1);
    yield helper(2);
    yield helper(3);
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "101 102 103 ");
}

#[test]
fn test_generator_calls_user_function_in_arithmetic() {
    let out = compile_and_run(
        r#"<?php
function dbl(int $x): int { return $x * 2; }
function gen() {
    $i = 1;
    while ($i < 5) {
        yield dbl($i) + 10;
        $i++;
    }
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "12 14 16 18 ");
}

#[test]
fn test_generator_send_int_arg_routes_into_yield_assign() {
    // `Generator::send($v)` stashes the boxed Mixed pointer in the
    // sent_value slot; the YieldAssign resume path unboxes it back to an
    // int and stores it into the assignment LHS local. Subsequent
    // `current()` reflects whatever the generator yields after that.
    let out = compile_and_run(
        r#"<?php
function echoer() {
    $a = yield 1;
    $b = yield $a;
    yield $b;
}
$g = echoer();
$g->rewind();
echo $g->current(); echo " ";
$g->send(100);
echo $g->current(); echo " ";
$g->send(200);
echo $g->current();
"#,
    );
    assert_eq!(out, "1 100 200");
}

#[test]
fn test_generator_yields_string_values() {
    let out = compile_and_run(
        r#"<?php
function gen() {
    yield "alpha";
    yield "beta";
    yield "gamma";
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "alpha beta gamma ");
}

#[test]
fn test_generator_yields_int_literals() {
    // A generator function with `yield <int_literal>` statements produces
    // those values when iterated with foreach. The state-machine codegen
    // emits a wrapper that allocates a GeneratorFrame plus a resume
    // function that drives the body across yield points.
    let out = compile_and_run(
        r#"<?php
function gen() {
    yield 1;
    yield 2;
}
foreach (gen() as $v) {
    echo $v;
}
echo "done";
"#,
    );
    assert_eq!(out, "12done");
}

#[test]
fn test_generator_yields_three_values() {
    let out = compile_and_run(
        r#"<?php
function gen() {
    yield 10;
    yield 20;
    yield 30;
}
foreach (gen() as $v) {
    echo $v;
    echo " ";
}
"#,
    );
    assert_eq!(out, "10 20 30 ");
}

#[test]
fn test_generator_yields_with_explicit_int_keys() {
    let out = compile_and_run(
        r#"<?php
function gen() {
    yield 100 => 1;
    yield 200 => 2;
    yield 300 => 3;
}
foreach (gen() as $k => $v) {
    echo $k;
    echo ":";
    echo $v;
    echo " ";
}
"#,
    );
    assert_eq!(out, "100:1 200:2 300:3 ");
}

#[test]
fn test_generator_auto_incrementing_keys() {
    let out = compile_and_run(
        r#"<?php
function gen() {
    yield 5;
    yield 6;
    yield 7;
}
foreach (gen() as $k => $v) {
    echo $k;
    echo "=>";
    echo $v;
    echo " ";
}
"#,
    );
    assert_eq!(out, "0=>5 1=>6 2=>7 ");
}

#[test]
fn test_generator_yields_int_parameters() {
    let out = compile_and_run(
        r#"<?php
function gen(int $a, int $b) {
    yield $a;
    yield $b;
    yield $a;
}
foreach (gen(7, 9) as $v) {
    echo $v;
    echo " ";
}
"#,
    );
    assert_eq!(out, "7 9 7 ");
}

#[test]
fn test_generator_yields_const_folded_arithmetic() {
    let out = compile_and_run(
        r#"<?php
function gen() {
    yield 1 + 2;
    yield 3 * 4;
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "3 12 ");
}

#[test]
fn test_generator_yields_param_arithmetic() {
    let out = compile_and_run(
        r#"<?php
function gen(int $a) {
    yield $a + 1;
    yield $a * 2;
}
foreach (gen(10) as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "11 20 ");
}

#[test]
fn test_generator_local_variable_across_yields() {
    let out = compile_and_run(
        r#"<?php
function gen() {
    $x = 5;
    yield $x;
    $x = 10;
    yield $x;
    $x = 99;
    yield $x;
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "5 10 99 ");
}

#[test]
fn test_generator_counter_with_arithmetic_assignment() {
    let out = compile_and_run(
        r#"<?php
function gen() {
    $i = 0;
    yield $i;
    $i = $i + 1;
    yield $i;
    $i = $i + 1;
    yield $i;
    $i = $i + 1;
    yield $i;
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 1 2 3 ");
}

#[test]
fn test_generator_post_increment_local() {
    let out = compile_and_run(
        r#"<?php
function gen() {
    $i = 10;
    yield $i;
    $i++;
    yield $i;
    $i++;
    yield $i;
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "10 11 12 ");
}

#[test]
fn test_generator_with_while_loop() {
    let out = compile_and_run(
        r#"<?php
function gen() {
    $i = 0;
    while ($i < 5) {
        yield $i;
        $i++;
    }
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 1 2 3 4 ");
}

#[test]
fn test_generator_with_if_else() {
    let out = compile_and_run(
        r#"<?php
function gen(int $n) {
    if ($n > 5) {
        yield 100;
    } else {
        yield 200;
    }
    yield $n;
}
foreach (gen(10) as $v) { echo $v; echo " "; }
echo "| ";
foreach (gen(3) as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "100 10 | 200 3 ");
}

#[test]
fn test_generator_with_for_loop() {
    let out = compile_and_run(
        r#"<?php
function gen() {
    for ($i = 0; $i < 5; $i++) {
        yield $i;
    }
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 1 2 3 4 ");
}

#[test]
fn test_generator_break_in_for() {
    let out = compile_and_run(
        r#"<?php
function gen() {
    for ($i = 0; $i < 100; $i++) {
        if ($i == 5) { break; }
        yield $i;
    }
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 1 2 3 4 ");
}

#[test]
fn test_generator_continue_in_for_runs_update() {
    // `continue` must jump to the for-loop's update step, NOT the loop top —
    // otherwise $i would never increment past 3 and the generator hangs.
    let out = compile_and_run(
        r#"<?php
function gen() {
    for ($i = 0; $i < 10; $i++) {
        if ($i == 3) { continue; }
        if ($i == 7) { continue; }
        yield $i;
    }
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 1 2 4 5 6 8 9 ");
}

#[test]
fn test_generator_elseif_chain() {
    let out = compile_and_run(
        r#"<?php
function classify(int $n) {
    if ($n < 0) {
        yield 0 - 1;
    } elseif ($n == 0) {
        yield 0;
    } elseif ($n < 10) {
        yield 1;
    } else {
        yield 100;
    }
}
foreach (classify(0 - 5) as $v) { echo $v; echo " "; }
foreach (classify(0) as $v) { echo $v; echo " "; }
foreach (classify(7) as $v) { echo $v; echo " "; }
foreach (classify(50) as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "-1 0 1 100 ");
}

#[test]
fn test_generator_nested_for_with_break() {
    let out = compile_and_run(
        r#"<?php
function gen() {
    for ($i = 0; $i < 3; $i++) {
        for ($j = 0; $j < 3; $j++) {
            if ($j == 2) { break; }
            yield $i * 10 + $j;
        }
    }
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 1 10 11 20 21 ");
}

#[test]
fn test_generator_do_while() {
    let out = compile_and_run(
        r#"<?php
function gen() {
    $i = 0;
    do {
        yield $i;
        $i++;
    } while ($i < 3);
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 1 2 ");
}

#[test]
fn test_generator_fibonacci() {
    let out = compile_and_run(
        r#"<?php
function fib(int $count) {
    $a = 0;
    $b = 1;
    $i = 0;
    while ($i < $count) {
        yield $a;
        $c = $a + $b;
        $a = $b;
        $b = $c;
        $i++;
    }
}
foreach (fib(10) as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 1 1 2 3 5 8 13 21 34 ");
}

#[test]
fn test_generator_combined_param_key_and_value() {
    let out = compile_and_run(
        r#"<?php
function gen(int $start, int $end) {
    yield $start => 1;
    yield $end => 2;
    yield 99 => $start;
}
foreach (gen(10, 20) as $k => $v) {
    echo $k;
    echo "->";
    echo $v;
    echo " ";
}
"#,
    );
    assert_eq!(out, "10->1 20->2 99->10 ");
}

#[test]
fn test_generator_throw_propagates_to_caller_catch() {
    // `Generator::throw($exc)` sets TERMINATED, publishes the exception
    // in the global slot, and tail-calls the unwinder; the catch in the
    // caller picks it up.
    let out = compile_and_run(
        r#"<?php
function gen() {
    yield 1;
    yield 2;
}
try {
    $g = gen();
    $g->rewind();
    echo $g->current();
    echo " ";
    $g->throw(new Exception("boom"));
    echo "unreachable";
} catch (Exception $e) {
    echo "caught: ";
    echo $e->getMessage();
}
"#,
    );
    assert_eq!(out, "1 caught: boom");
}

#[test]
fn test_generator_yield_string_from_local_slot() {
    // A local assigned a string literal becomes a Mixed-typed slot;
    // yielding the local incref's the boxed cell so both the slot and
    // the outer `last_value` keep refcounts. Re-assigning the slot
    // refcount-replaces the cell.
    let out = compile_and_run(
        r#"<?php
function gen() {
    $a = "first";
    yield $a;
    $a = "second";
    yield $a;
    $a = "third";
    yield $a;
}
foreach (gen() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "first second third ");
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
fn test_generator_send_with_string_payload_into_mixed_slot() {
    // `Generator::send($v)` with a string payload now lands in a
    // Mixed-typed local slot via refcount transfer (no unboxing). The
    // generator alternates between `yield <prompt>` and `yield $reply`.
    let out = compile_and_run(
        r#"<?php
function gen() {
    $x = "init";
    $x = yield "first";
    yield $x;
    $x = yield "second";
    yield $x;
}
$g = gen();
$g->rewind();
echo $g->current(); echo " ";
$g->send("alpha");
echo $g->current(); echo " ";
$g->send("beta");
echo $g->current(); echo " ";
$g->send("gamma");
echo $g->current();
"#,
    );
    assert_eq!(out, "first alpha second gamma");
}

#[test]
fn test_generator_yield_int_array_local_slot() {
    // A local assigned an int-array literal becomes Mixed-typed; the
    // generator can yield it without crashing or leaking.
    let out = compile_and_run(
        r#"<?php
function gen() {
    $arr = [1, 2, 3];
    yield $arr;
    $arr = [10, 20];
    yield $arr;
}
foreach (gen() as $v) { echo "got "; }
"#,
    );
    assert_eq!(out, "got got ");
}

#[test]
fn test_foreach_iterator_aggregate_class() {
    // A class that implements only IteratorAggregate (not Iterator
    // directly) — foreach calls getIterator() once before the loop and
    // dispatches the per-iteration calls against the returned class.
    let out = compile_and_run(
        r#"<?php
class Range implements Iterator {
    private int $current;
    private int $end;
    public function __construct(int $start, int $end) {
        $this->current = $start;
        $this->end = $end;
    }
    public function rewind(): void {}
    public function valid(): bool { return $this->current < $this->end; }
    public function current(): mixed { return $this->current; }
    public function key(): mixed { return $this->current; }
    public function next(): void { $this->current = $this->current + 1; }
}
class Aggregate implements IteratorAggregate {
    public function getIterator(): Range { return new Range(0, 5); }
}
foreach (new Aggregate() as $v) { echo $v; echo " "; }
"#,
    );
    assert_eq!(out, "0 1 2 3 4 ");
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

#[test]
fn test_generator_yields_int_array_literal() {
    // `yield [1, 2, 3]` — the consumer receives a Mixed-boxed indexed
    // array. We verify only that the generator runs to completion past
    // the array yield (count() on Mixed is a separate concern).
    let out = compile_and_run(
        r#"<?php
function gen() {
    yield [1, 2, 3];
    yield [10, 20];
}
foreach (gen() as $arr) {
    echo "ok ";
}
"#,
    );
    assert_eq!(out, "ok ok ");
}

#[test]
fn test_generator_return_value_via_get_return() {
    // `return $v;` inside a generator stashes $v in the frame's
    // return_value slot and terminates. `getReturn()` retrieves it.
    let out = compile_and_run(
        r#"<?php
function gen() {
    yield 1;
    yield 2;
    return 42;
}
$g = gen();
foreach ($g as $v) { echo $v; echo " "; }
echo "ret=";
echo $g->getReturn();
"#,
    );
    assert_eq!(out, "1 2 ret=42");
}

#[test]
fn test_generator_bare_return_terminates() {
    // `return;` (no value) terminates the generator without writing a
    // return value. The previously zero-initialised return_value cell
    // surfaces as null/0.
    let out = compile_and_run(
        r#"<?php
function gen() {
    yield 1;
    return;
    yield 99;
}
foreach (gen() as $v) { echo $v; echo " "; }
echo "done";
"#,
    );
    assert_eq!(out, "1 done");
}

#[test]
fn test_foreach_user_iterator_break() {
    let out = compile_and_run(
        r#"<?php
class Counter implements Iterator {
    private int $i;
    public function __construct() { $this->i = 0; }
    public function rewind(): void {}
    public function valid(): bool { return true; }
    public function current(): mixed { return $this->i; }
    public function key(): mixed { return $this->i; }
    public function next(): void { $this->i = $this->i + 1; }
}
foreach (new Counter() as $v) {
    if ($v == 4) { break; }
    echo $v;
}
"#,
    );
    assert_eq!(out, "0123");
}
