<?php
$it = new ArrayIterator(["alpha" => 10, "beta" => 20]);
$it["gamma"] = 30;

foreach ($it as $key => $value) {
    echo $key;
    echo "=";
    echo $value;
    echo "\n";
}

$obj = new ArrayObject(["left" => "L", "right" => "R"]);
foreach ($obj as $key => $value) {
    echo $key;
    echo ":";
    echo $value;
    echo "\n";
}

