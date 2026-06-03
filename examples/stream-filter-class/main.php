<?php
// User-defined stream filters let PHP programs intercept the bytes that
// flow through a stream and transform them on read or write. PHP's real
// API uses bucket brigades (`filter($in, $out, &$consumed, $closing)`),
// but elephc v1 uses a simpler `filter(string $data): string` contract
// — one buffer in, one buffer out, no `$consumed`/`$closing` semantics
// and no multi-bucket aggregation.
//
// Wire-up:
//   - stream_filter_register("name", "Class") records the (name, class)
//     pair in the runtime filter registry (capacity 128).
//   - stream_filter_append/prepend($stream, "name", $direction) resolves
//     the name through the registry, instantiates the class via dynamic
//     `new`, caches the instance per (fd, direction), and points the
//     per-fd filter byte at the user id.
//   - On every fread / fwrite, the runtime checks the per-fd filter id;
//     IDs >= 128 dispatch into the cached instance's `filter()` method.

class UpperFilter {
    public function filter(string $data): string {
        return strtoupper($data);
    }
}

class ReverseFilter {
    public function filter(string $data): string {
        return strrev($data);
    }
}

stream_filter_register("user.upper", "UpperFilter");
stream_filter_register("user.reverse", "ReverseFilter");

// Write through an upper-casing filter, then read back unchanged.
$f = fopen("php://memory", "r+");
stream_filter_append($f, "user.upper", STREAM_FILTER_WRITE);
fwrite($f, "hello world");
rewind($f);
echo "write upper: " . fread($f, 64) . "\n";
fclose($f);

// Read through a reversing filter — bytes on the stream are unchanged,
// the read path sees the transformed result.
$g = fopen("php://memory", "r+");
fwrite($g, "abcdef");
rewind($g);
stream_filter_append($g, "user.reverse", STREAM_FILTER_READ);
echo "read reverse: " . fread($g, 64) . "\n";
fclose($g);

// Unregistered filter names resolve to PHP false at attach time.
$h = fopen("php://memory", "r+");
$r = stream_filter_append($h, "this.does.not.exist");
echo "missing filter: " . ($r === false ? "false" : "ok") . "\n";
fclose($h);
