<?php
// The gz* builtins compress and decompress strings with the system zlib
// library. Compression shrinks repetitive data substantially.

$original = str_repeat("elephc compiles PHP to native code. ", 8);
echo "original size:   " . strlen($original) . "\n";

// gzcompress() / gzuncompress() use the zlib-wrapped format (header + Adler32).
$compressed = gzcompress($original);
echo "compressed size: " . strlen($compressed) . "\n";

$restored = gzuncompress($compressed);
echo "restored matches original: " . ($restored === $original ? "yes" : "no") . "\n";

// gzdeflate() / gzinflate() use raw DEFLATE — no header or trailer, so the
// output is a few bytes smaller. This is also what the zlib.deflate stream
// filter produces.
$deflated = gzdeflate($original);
echo "raw deflated size: " . strlen($deflated) . "\n";

$inflated = gzinflate($deflated);
echo "inflated matches original: " . ($inflated === $original ? "yes" : "no") . "\n";
