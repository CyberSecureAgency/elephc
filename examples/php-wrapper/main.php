<?php
// The php:// wrapper opens the process's standard streams by name.

$stdout = fopen("php://stdout", "w");
fwrite($stdout, "hello from php://stdout\n");

$stderr = fopen("php://stderr", "w");
fwrite($stderr, "warnings go to php://stderr\n");

// php://output is an alias of stdout; php://input aliases stdin.
$out = fopen("php://output", "w");
fwrite($out, "php://output writes here too\n");

echo "the handle is a real resource: " . get_resource_type($stdout) . "\n";
