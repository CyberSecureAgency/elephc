<?php
interface Renderable {
    public function render();
}

class Widget {
    public function render() {
        return "widget";
    }
}

class Button extends Widget implements Renderable {
    public function label() {
        return "button";
    }
}

class Probe extends Widget {
    public function check(Widget $item) {
        echo ($item instanceof static) ? "late\n" : "not late\n";
    }
}

$item = new Button();
echo ($item instanceof Button) ? "button\n" : "not button\n";
echo ($item instanceof Widget) ? "widget\n" : "not widget\n";
echo ($item instanceof Renderable) ? "renderable\n" : "not renderable\n";
echo ($item instanceof Missing) ? "missing\n" : "not missing\n";

$className = "Button";
$interfaceName = "Renderable";
$targetObject = new Widget();
echo ($item instanceof $className) ? "dynamic class\n" : "not dynamic class\n";
echo ($item instanceof $interfaceName) ? "dynamic interface\n" : "not dynamic interface\n";
echo ($item instanceof $targetObject) ? "dynamic object\n" : "not dynamic object\n";

$probe = new Probe();
$probe->check($item);
$probe->check(new Probe());
