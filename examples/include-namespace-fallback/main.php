<?php

namespace App;

use function App\Lib\helper;

require __DIR__ . "/lib.php";

echo function_exists("helper") ? "y\n" : "n\n";
echo function_exists("\\App\\Lib\\helper") ? "y\n" : "n\n";
echo helper() . "\n";
