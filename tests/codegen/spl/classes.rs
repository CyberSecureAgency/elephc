//! Purpose:
//! End-to-end tests for built-in SPL container classes.
//! Verifies Phase 4 container metadata plus runtime-backed list behavior.
//!
//! Called from:
//! - `cargo test --test codegen_tests` through the SPL test module.
//!
//! Key details:
//! - Runtime tests cover Phase 4 containers before later SPL phases add iterator decorators and heaps.

use crate::support::*;

#[test]
fn test_phase4_spl_classes_are_declared_for_introspection() {
    let out = compile_and_run(
        r#"<?php
function has_name(array $names, string $target): bool {
    foreach ($names as $name) {
        if ($name === $target) {
            return true;
        }
    }
    return false;
}

$spl = spl_classes();
echo has_name($spl, "SplDoublyLinkedList");
echo has_name($spl, "SplStack");
echo has_name($spl, "SplQueue");
echo has_name($spl, "SplFixedArray");

$declared = get_declared_classes();
echo has_name($declared, "SplDoublyLinkedList");
echo has_name($declared, "SplStack");
echo has_name($declared, "SplQueue");
echo has_name($declared, "SplFixedArray");

var_dump(class_exists("SplDoublyLinkedList"));
var_dump(class_exists("splstack"));
"#,
    );
    assert_eq!(out, "11111111bool(true)\nbool(true)\n");
}

#[test]
fn test_phase4_spl_class_interface_and_parent_metadata() {
    let out = compile_and_run(
        r#"<?php
$list = new SplDoublyLinkedList();
var_dump($list instanceof Iterator);
var_dump($list instanceof Countable);
var_dump($list instanceof ArrayAccess);

$stack = new SplStack();
var_dump($stack instanceof SplDoublyLinkedList);
var_dump($stack instanceof Iterator);

$queue = new SplQueue();
var_dump($queue instanceof SplDoublyLinkedList);
var_dump($queue instanceof Countable);

$fixed = new SplFixedArray();
var_dump($fixed instanceof ArrayAccess);
var_dump($fixed instanceof Countable);
var_dump($fixed instanceof JsonSerializable);
"#,
    );
    assert_eq!(
        out,
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn test_phase4_spl_doubly_linked_list_constants_are_inherited() {
    let out = compile_and_run(
        r#"<?php
echo SplDoublyLinkedList::IT_MODE_LIFO;
echo ",";
echo SplStack::IT_MODE_DELETE;
echo ",";
echo SplQueue::IT_MODE_FIFO;
"#,
    );
    assert_eq!(out, "2,1,0");
}

#[test]
fn test_phase4_spl_doubly_linked_list_mutation_methods() {
    let out = compile_and_run(
        r#"<?php
$list = new SplDoublyLinkedList();
var_dump($list->isEmpty());
$list->push("a");
$list->push(2);
$list->unshift("z");
$list->add(1, "m");
echo count($list);
echo "\n";
echo $list->bottom();
echo "|";
echo $list->top();
echo "\n";
echo $list->shift();
echo "|";
echo $list->pop();
echo "|";
echo $list->shift();
echo "|";
echo $list->pop();
echo "\n";
var_dump($list->isEmpty());
"#,
    );
    assert_eq!(
        out,
        concat!(
            "bool(true)\n",
            "4\n",
            "z|2\n",
            "z|2|m|a\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn test_phase4_spl_doubly_linked_list_iteration_modes() {
    let out = compile_and_run(
        r#"<?php
$list = new SplDoublyLinkedList();
$list->push("a");
$list->push("b");
$list->push("c");
foreach ($list as $k => $v) {
    echo $k;
    echo ":";
    echo $v;
    echo ";";
}
echo "\n";
$list->setIteratorMode(SplDoublyLinkedList::IT_MODE_LIFO);
foreach ($list as $k => $v) {
    echo $k;
    echo ":";
    echo $v;
    echo ";";
}
echo "\n";
echo $list->getIteratorMode();
"#,
    );
    assert_eq!(out, "0:a;1:b;2:c;\n2:c;1:b;0:a;\n2");
}

#[test]
fn test_phase4_spl_doubly_linked_list_delete_iteration_modes() {
    let out = compile_and_run(
        r#"<?php
$fifo = new SplDoublyLinkedList();
$fifo->push("a");
$fifo->push("b");
$fifo->push("c");
$fifo->setIteratorMode(SplDoublyLinkedList::IT_MODE_FIFO | SplDoublyLinkedList::IT_MODE_DELETE);
foreach ($fifo as $k => $v) {
    echo $k;
    echo ":";
    echo $v;
    echo ";";
}
echo "\n";
echo count($fifo);
echo "\n";

$lifo = new SplDoublyLinkedList();
$lifo->push("a");
$lifo->push("b");
$lifo->push("c");
$lifo->setIteratorMode(SplDoublyLinkedList::IT_MODE_LIFO | SplDoublyLinkedList::IT_MODE_DELETE);
foreach ($lifo as $k => $v) {
    echo $k;
    echo ":";
    echo $v;
    echo ";";
}
echo "\n";
echo count($lifo);
"#,
    );
    assert_eq!(out, "0:a;0:b;0:c;\n0\n2:c;1:b;0:a;\n0");
}

#[test]
fn test_phase4_spl_doubly_linked_list_array_access() {
    let out = compile_and_run(
        r#"<?php
$list = new SplDoublyLinkedList();
$list[] = "a";
$list[] = "b";
echo $list[0];
echo "|";
echo $list[1];
echo "\n";
echo isset($list[1]);
echo "\n";
unset($list[0]);
echo $list[0];
echo "\n";
$list[1] = "c";
echo $list[1];
echo "\n";
$list[0] = "z";
echo $list[0];
echo "|";
echo count($list);
"#,
    );
    assert_eq!(out, "a|b\n1\nb\nc\nz|2");
}

#[test]
fn test_phase4_spl_stack_and_queue_runtime_methods() {
    let out = compile_and_run(
        r#"<?php
$stack = new SplStack();
$stack->push(1);
$stack->push(2);
echo $stack->pop();
echo "|";
echo $stack->top();
echo "|";
echo count($stack);
echo "\n";

$queue = new SplQueue();
$queue->enqueue("a");
$queue->enqueue("b");
echo $queue->dequeue();
echo "|";
echo $queue->bottom();
echo "|";
echo $queue->top();
echo "|";
echo count($queue);
"#,
    );
    assert_eq!(out, "2|1|1\na|b|b|1");
}

#[test]
fn test_phase4_spl_fixed_array_runtime_methods() {
    let out = compile_and_run(
        r#"<?php
$fixed = new SplFixedArray(2);
echo count($fixed);
echo "|";
echo $fixed->getSize();
echo "\n";
$fixed[0] = "a";
$fixed[1] = 3;
echo $fixed[0];
echo "|";
echo $fixed[1];
echo "\n";
echo isset($fixed[0]);
unset($fixed[0]);
echo isset($fixed[0]);
echo "\n";
$fixed->setSize(3);
$fixed[2] = "c";
echo count($fixed);
echo "|";
echo $fixed[2];
echo "\n";
$array = $fixed->toArray();
echo count($array);
echo "|";
echo $array[1];
echo "|";
echo $array[2];
echo "\n";
$json = $fixed->jsonSerialize();
echo count($json);
"#,
    );
    assert_eq!(out, "2|2\na|3\n10\n3|c\n3|3|c\n3");
}
