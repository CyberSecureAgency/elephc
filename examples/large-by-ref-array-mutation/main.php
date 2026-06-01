<?php

function mutate_late_by_ref_array(
    $p1,
    $p2,
    $p3,
    $p4,
    $p5,
    $p6,
    $p7,
    $p8,
    $p9,
    $p10,
    $p11,
    $p12,
    $p13,
    $p14,
    $p15,
    &$arr
): void {
    $guard = $p1 + $p2 + $p3 + $p4 + $p5 + $p6 + $p7 + $p8 + $p9 + $p10
        + $p11 + $p12 + $p13 + $p14 + $p15;
    if ($guard === 0) {
        echo "unreachable";
    }
    $arr[0] = 41;
    $arr[1] = 42;
}

$values = [0, 0];
mutate_late_by_ref_array(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, $values);
echo $values[0] . "|" . $values[1] . "\n";
