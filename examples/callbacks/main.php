<?php

// Callback-based array functions demo

function double($x) { return $x * 2; }
function is_positive($x) { return $x > 0; }
function sum($carry, $item) { return $carry + $item; }
function compare($a, $b) { return $a - $b; }
function show($x) { echo "  " . $x . "\n"; }

$numbers = [3, -1, 4, -5, 2, -3, 1];

// array_map: transform each element
$doubled = array_map("double", $numbers);
echo "Doubled: ";
foreach ($doubled as $v) { echo $v . " "; }
echo "\n";

// array_filter: keep only matching elements
$positives = array_filter($numbers, "is_positive");
echo "Positives: ";
foreach ($positives as $v) { echo $v . " "; }
echo "\n";

// array_reduce: fold into a single value
$total = array_reduce($numbers, "sum", 0);
echo "Sum: " . $total . "\n";

// usort: sort with custom comparator
$sorted = [5, 2, 8, 1, 9];
usort($sorted, "compare");
echo "Sorted: ";
foreach ($sorted as $v) { echo $v . " "; }
echo "\n";

// array_walk: apply side-effect to each element
echo "Walk:\n";
$items = [10, 20, 30];
array_walk($items, "show");

// call_user_func: indirect function call
$result = call_user_func("double", 21);
echo "call_user_func(double, 21) = " . $result . "\n";

class Formatter {
    public function bracket(string $value): string {
        return "[" . $value . "]";
    }
}

$formatter = new Formatter();
$format = $formatter->bracket(...);
echo "method callable: " . $format("ok") . "\n";
$formatted = array_map($format, ["a", "b"]);
echo "method callable array_map: ";
foreach ($formatted as $v) { echo $v . " "; }
echo "\n";
echo "method callable call_user_func_array: " . call_user_func_array($format, ["cb"]) . "\n";

class Labeler {
    public static function current() {
        $label = static::name(...);
        return $label();
    }

    public static function name() {
        return "base";
    }
}

class LoudLabeler extends Labeler {
    public static function name() {
        return "loud";
    }
}

echo "static callable: " . Labeler::current() . "/" . LoudLabeler::current() . "\n";

// function_exists: check if a function is defined
if (function_exists("double")) {
    echo "function 'double' exists\n";
}
if (!function_exists("nonexistent")) {
    echo "function 'nonexistent' does not exist\n";
}

// is_callable: dynamic strings, method arrays, static method arrays, and invokable objects
class Runner {
    public function run() {
        return "running";
    }
}

class InvokableRunner {
    public function __invoke() {
        return "invoked";
    }
}

class StaticRunner {
    public static function run() {
        return "static";
    }
}

$callback_name = "double";
$static_callback_name = "StaticRunner::run";
$runner = new Runner();
$method_callback = [$runner, "run"];
$static_method_callback = [StaticRunner::class, "run"];
$invokable_runner = new InvokableRunner();

echo "is_callable dynamic string: " . (is_callable($callback_name) ? "yes" : "no") . "\n";
echo "is_callable static string: " . (is_callable($static_callback_name) ? "yes" : "no") . "\n";
echo "is_callable method array: " . (is_callable($method_callback) ? "yes" : "no") . "\n";
echo "is_callable static method array: " . (is_callable($static_method_callback) ? "yes" : "no") . "\n";
echo "is_callable invokable object: " . (is_callable($invokable_runner) ? "yes" : "no") . "\n";
