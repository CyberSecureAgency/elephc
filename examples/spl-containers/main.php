<?php

$list = new SplDoublyLinkedList();
$list->push("alpha");
$list->push("beta");
$list->unshift("start");

foreach ($list as $index => $value) {
    echo $index;
    echo ": ";
    echo $value;
    echo "\n";
}

$queue = new SplQueue();
$queue->enqueue("first");
$queue->enqueue("second");
echo "queue: ";
echo $queue->dequeue();
echo "\n";

$stack = new SplStack();
$stack->push(10);
$stack->push(20);
echo "stack: ";
echo $stack->pop();
echo "\n";

$fixed = new SplFixedArray(2);
$fixed[0] = "left";
$fixed[1] = "right";
echo "fixed: ";
echo $fixed[0];
echo ", ";
echo $fixed[1];
echo "\n";
