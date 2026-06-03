<?php
// User-defined stream wrappers let PHP programs register a custom class
// against a URL scheme; the class's stream_open / stream_read / stream_write
// / stream_close / stream_eof / stream_seek methods then handle the requested
// fopen/fread/fwrite/fclose/feof/fseek calls.
//
// elephc dispatches into the wrapper class through the regular method ABI.
// Wrapper methods should declare their return types so the runtime call lands
// in the expected result register: bool for stream_open / stream_eof /
// stream_seek, string for stream_read, int for stream_write, void for
// stream_close.

class MemoryStream {
    public string $buffer = "";
    public int $pos = 0;

    public function stream_open(string $path, string $mode, int $options, ?string &$opened_path): bool {
        // A real wrapper would parse $path/$mode/$options/$opened_path here.
        // This demo accepts every memory://... URL.
        echo "open: " . $path . "(" . $mode . "," . $options . ")\n";
        $this->buffer = "hello from a user-defined wrapper";
        $this->pos = 0;
        return true;
    }

    public function stream_read(int $count): string {
        $remaining = strlen($this->buffer) - $this->pos;
        if ($remaining <= 0) {
            return "";
        }
        if ($count > $remaining) {
            $count = $remaining;
        }
        $slice = substr($this->buffer, $this->pos, $count);
        $this->pos = $this->pos + $count;
        return $slice;
    }

    public function stream_eof(): bool {
        return $this->pos >= strlen($this->buffer);
    }

    public function stream_seek(int $offset, int $whence): bool {
        // A real wrapper would honor $whence (SEEK_SET / SEEK_CUR / SEEK_END);
        // this demo always seeks from the start.
        echo "seek($offset, $whence)\n";
        $this->pos = $offset;
        return true;
    }

    public function stream_close(): void {
        // Nothing to release: the buffer lives on the PHP heap.
    }

    public function stream_write(string $data): int {
        // fwrite() and fputcsv() route here. Echo the bytes so the demo shows
        // exactly what reached the wrapper; a real wrapper would buffer/forward.
        echo $data;
        return strlen($data);
    }

    // stream_stat() backs fstat() on an open wrapper handle, and url_stat()
    // backs the path-based file_exists() / filesize() / is_file(). Both return
    // a PHP stat array; declare them WITHOUT a return type (a `: array` return
    // would be integer-keyed and reject the string keys a stat array uses).
    public function stream_stat() {
        return ['size' => strlen($this->buffer), 'mode' => 33188 /* S_IFREG|0644 */];
    }

    public function url_stat(string $path, int $flags) {
        // Report the demo URL as an existing 33-byte regular file.
        if (strpos($path, "demo") !== false) {
            return ['size' => 33, 'mode' => 33188];
        }
        return false;
    }
}

stream_wrapper_register("memory", "MemoryStream");

$f = fopen("memory://demo", "r");
echo fread($f, 5);                    // "hello"
echo "|";
echo fread($f, 100);                  // " from a user-defined wrapper"
echo "|";
echo feof($f) ? "eof" : "more";       // "eof"
echo "|";
fseek($f, 0, 0);
echo fread($f, 5);                    // "hello" again
fclose($f);
echo "\n";

// The whole-stream read builtins also drain a userspace wrapper. They check
// the wrapper's stream_eof() before each stream_read(), so they stop cleanly
// at end-of-stream.
$g = fopen("memory://demo", "r");
echo "all: " . stream_get_contents($g) . "\n";
fclose($g);

$h = fopen("memory://demo", "r");
echo "passthru: ";
fpassthru($h);                        // streams the wrapper's bytes to stdout
echo "\n";
fclose($h);

// fstat() on an open handle dispatches into the wrapper's stream_stat().
$s = fopen("memory://demo", "r");
$st = fstat($s);
echo "fstat size: " . $st['size'] . "\n";   // 33
fclose($s);

// The path-based stat builtins dispatch into url_stat() for "memory://" URLs,
// and fall back to the real filesystem for ordinary paths.
echo "exists: " . (file_exists("memory://demo") ? "yes" : "no") . "\n";   // yes
echo "filesize: " . filesize("memory://demo") . "\n";                     // 33
echo "is_file: " . (is_file("memory://demo") ? "yes" : "no") . "\n";      // yes

// readfile() streams a wrapper URL to stdout (fopen + stream_read drain + close).
echo "readfile: ";
readfile("memory://demo");            // "hello from a user-defined wrapper"
echo "\n";

// stream_get_line() reads up to a delimiter from a wrapper handle.
$w = fopen("memory://demo", "r");
echo "first word: " . stream_get_line($w, 100, " ") . "\n";   // "hello"
fclose($w);

// fputcsv() routes each field/separator/quote/newline segment through the
// wrapper's stream_write(). The second row's first field embeds a comma, so
// fputcsv() CSV-quotes it ("Ada, Countess").
echo "--- fputcsv through a wrapper ---\n";
$c = fopen("memory://csv", "w");
fputcsv($c, ["name", "age"]);
fputcsv($c, ["Ada, Countess", "36"]);
fclose($c);
