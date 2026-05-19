<?php
function base() {
    yield 1;
    yield 2;
    return 99;
}

function pipeline() {
    yield 0;
    return yield from base();
}

$g = pipeline();
foreach ($g as $k => $v) {
    echo $k;
    echo ":";
    echo $v;
    echo " ";
}
echo "ret=";
echo $g->getReturn();
echo "\n";

$start = 5;
$make = function() use ($start) {
    yield $start;
    yield $start + 1;
};

foreach ($make() as $v) {
    echo $v;
    echo " ";
}
echo "\n";

function responder() {
    $value = yield 10;
    yield $value;
    return 77;
}

$r = responder();
$r->rewind();
echo $r->current();
echo " ";
$r->send(42);
echo $r->current();
echo " ";
$r->next();
echo $r->getReturn();
echo "\n";
