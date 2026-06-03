<?php
// Stream filters transform data as it passes through a stream. A filter is
// attached with stream_filter_append() and acts on reads, writes, or both.

// A write filter upper-cases everything written into the stream.
$out = fopen("php://memory", "r+");
stream_filter_append($out, "string.toupper", STREAM_FILTER_WRITE);
fwrite($out, "data written in lower case");
rewind($out);
echo "stored: " . fread($out, 64) . "\n";
fclose($out);

// A read filter rot13-encodes everything read back out of the stream.
$in = fopen("php://memory", "r+");
fwrite($in, "Secret Message");
rewind($in);
stream_filter_append($in, "string.rot13", STREAM_FILTER_READ);
echo "rot13:  " . fread($in, 64) . "\n";
fclose($in);

// The zlib.deflate write filter compresses everything written through the
// stream with raw DEFLATE. Repetitive data shrinks substantially. The 4th
// $params arg tunes the filter: here the canonical array form requests the
// maximum compression level (a bare `9` works too).
$path = __DIR__ . "/compressed.bin";
$packed = fopen($path, "w");
stream_filter_append($packed, "zlib.deflate", STREAM_FILTER_WRITE, ["level" => 9]);
$payload = str_repeat("stream filters compress on the fly. ", 12);
fwrite($packed, $payload);
fclose($packed);
echo "deflate: " . strlen($payload) . " bytes -> "
   . strlen(file_get_contents($path)) . " bytes\n";

// The zlib.inflate read filter decompresses the stream as it is read back.
$unpack = fopen($path, "r");
stream_filter_append($unpack, "zlib.inflate", STREAM_FILTER_READ);
$restored = stream_get_contents($unpack);
fclose($unpack);
echo "inflate: recovered " . strlen($restored) . " bytes"
   . ($restored === $payload ? " (matches)" : " (MISMATCH)") . "\n";

// The convert.iconv.<from>/<to> filter transcodes charsets through libc iconv.
// Here UTF-8 text is read back as UTF-16LE (two bytes per ASCII character).
$conv = fopen("php://memory", "r+");
fwrite($conv, "Hi");
rewind($conv);
stream_filter_append($conv, "convert.iconv.UTF-8/UTF-16LE", STREAM_FILTER_READ);
echo "iconv:  " . strlen(fread($conv, 64)) . " bytes from \"Hi\"\n";
fclose($conv);

// The bzip2.compress write filter compresses through libbz2; bzip2.decompress
// reads it back. The bzip2 stream format is interoperable with PHP's
// bzcompress()/bzdecompress() and the `bunzip2` tool.
// $params for bzip2.compress carries 'blocks' (blockSize100k) and 'work'
// (the workFactor) in the array form.
$bzpath = __DIR__ . "/bz2packed.bin";
$bzw = fopen($bzpath, "w");
stream_filter_append($bzw, "bzip2.compress", STREAM_FILTER_WRITE, ["blocks" => 9, "work" => 30]);
$bzpayload = str_repeat("bzip2 filters compress on the fly. ", 12);
fwrite($bzw, $bzpayload);
fclose($bzw);
echo "bzip2:  " . strlen($bzpayload) . " bytes -> " . strlen(file_get_contents($bzpath)) . " bytes";
$bzr = fopen($bzpath, "r");
stream_filter_append($bzr, "bzip2.decompress", STREAM_FILTER_READ);
$bzback = stream_get_contents($bzr);
fclose($bzr);
echo " -> recovered " . strlen($bzback) . ($bzback === $bzpayload ? " (matches)" : " (MISMATCH)") . "\n";

// The php://filter wrapper attaches a filter at fopen() time: it opens the
// underlying resource and applies the named filter to it.
$pf = fopen("php://filter/read=string.toupper/resource=php://temp", "r+");
fwrite($pf, "shout this");
rewind($pf);
echo "php://filter: " . fread($pf, 64) . "\n";
fclose($pf);

// The available built-in filters.
echo "filters: " . implode(", ", stream_get_filters()) . "\n";
