<?php

spl_autoload_register(function ($name) {
    if ($name === "Demo\\Thing") {
        require __DIR__ . "/DemoThing.php";
    }
});

if (class_exists("demo\\thing")) {
    $r = new ReflectionClass("DEMO\\THING");
    $attrs = $r->getAttributes();
    echo $r->getName() . "\n";
    echo count($attrs) . "\n";
    echo $attrs[0]->getName() . "\n";
}
