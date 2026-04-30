<?php

// Demonstrates implementing the built-in Iterator interface and consuming
// it from a foreach loop. elephc dispatches rewind/valid/current/key/next
// through the regular vtable — the same path PHP uses.

class Range implements Iterator {
    private int $current;
    private int $end;
    private int $step;

    public function __construct(int $start, int $end, int $step) {
        $this->current = $start;
        $this->end = $end;
        $this->step = $step;
    }

    public function rewind(): void {
        // Range has nothing to reset — `current` was set by __construct.
    }

    public function valid(): bool {
        return $this->current < $this->end;
    }

    public function current(): mixed {
        return $this->current;
    }

    public function key(): mixed {
        return $this->current;
    }

    public function next(): void {
        $this->current = $this->current + $this->step;
    }
}

echo "range 0..5 step 1:\n";
foreach (new Range(0, 5, 1) as $i) {
    echo $i;
    echo " ";
}
echo "\n";

echo "range 10..20 step 3:\n";
foreach (new Range(10, 20, 3) as $i) {
    echo $i;
    echo " ";
}
echo "\n";

echo "early exit when current >= 5:\n";
foreach (new Range(1, 100, 1) as $i) {
    if ($i == 5) {
        echo $i;
        echo "\n";
        break;
    }
}

// IteratorAggregate: the object itself isn't an iterator — its
// getIterator() method returns one. foreach calls getIterator() once
// before iterating.

class RangeFactory implements IteratorAggregate {
    private int $start;
    private int $end;

    public function __construct(int $start, int $end) {
        $this->start = $start;
        $this->end = $end;
    }

    public function getIterator(): Range {
        return new Range($this->start, $this->end, 1);
    }
}

echo "iterator aggregate 0..3:\n";
foreach (new RangeFactory(0, 3) as $i) {
    echo $i;
    echo " ";
}
echo "\n";
