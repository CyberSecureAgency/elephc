<?php
// http.php — HTTP/1.1 request parsing and response building.
//
// The parsing work is split across many small functions on purpose: a large
// function with many locals and many builtin calls can miscompile argument
// values in this elephc version, so each helper stays short and focused.

// strpos() that returns a plain int index, or -1 when the needle is absent.
// The intval() call produces a clean integer that is safe to feed straight
// into substr() — a raw strpos() result is not, in this elephc version.
function str_index(string $haystack, string $needle): int {
    $pos = strpos($haystack, $needle);
    if ($pos === false) {
        return -1;
    }
    return intval($pos);
}

// A parsed HTTP request.
class Request {
    public string $method = "";
    public string $path = "";
    public string $query = "";
    public string $version = "";
    public string $body = "";
    public string $head = "";   // raw request line + header lines

    // Look up a header by case-insensitive name, "" when absent.
    public function header(string $name): string {
        return find_header($this->head, $name);
    }
}

// An HTTP response under construction.
class Response {
    public int $status = 200;
    public string $ctype = "text/plain; charset=utf-8";
    public string $body = "";

    public function text(string $s): void {
        $this->ctype = "text/plain; charset=utf-8";
        $this->body = $s;
    }

    public function html(string $s): void {
        $this->ctype = "text/html; charset=utf-8";
        $this->body = $s;
    }

    public function json(string $s): void {
        $this->ctype = "application/json";
        $this->body = $s;
    }

    // Serialize into a complete HTTP/1.1 response message.
    public function render(): string {
        $out = "HTTP/1.1 " . $this->status . " " . status_text($this->status) . "\r\n";
        $out = $out . "Content-Type: " . $this->ctype . "\r\n";
        $out = $out . "Content-Length: " . strlen($this->body) . "\r\n";
        $out = $out . "Connection: close\r\n";
        $out = $out . "Server: elephc-http\r\n";
        $out = $out . "\r\n";
        return $out . $this->body;
    }
}

// Reason phrase for a status code.
function status_text(int $code): string {
    return match ($code) {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        default => "OK",
    };
}

// --- request parsing -------------------------------------------------------

// Parse a raw request whose header block is known to be complete.
function parse_request(string $raw): Request {
    $req = new Request();
    split_head_body($req, $raw);
    $lines = explode("\r\n", $req->head);
    parse_request_line($req, $lines[0]);
    return $req;
}

// Split the raw request into the header block ($req->head) and body.
function split_head_body(Request $req, string $raw): void {
    $split = str_index($raw, "\r\n\r\n");
    if ($split >= 0) {
        // intval() launders the computed offset so substr() receives it intact.
        $body_at = intval($split + 4);
        $req->head = substr($raw, 0, $split);
        $req->body = substr($raw, $body_at);
    } else {
        $req->head = $raw;
    }
}

// Parse the request line "METHOD request-target HTTP/1.1".
function parse_request_line(Request $req, string $line): void {
    $parts = explode(" ", $line);
    if (count($parts) >= 3) {
        $req->method = $parts[0];
        $req->version = $parts[2];
        parse_target($req, $parts[1]);
    }
}

// Split the request target into path and query string.
function parse_target(Request $req, string $target): void {
    $qpos = str_index($target, "?");
    if ($qpos >= 0) {
        $after = intval($qpos + 1);
        $req->path = substr($target, 0, $qpos);
        $req->query = substr($target, $after);
    } else {
        $req->path = $target;
    }
}

// --- header lookup ---------------------------------------------------------

// Find a header value by case-insensitive name in a raw header block.
function find_header(string $head, string $name): string {
    $want = strtolower($name);
    $lines = explode("\r\n", $head);
    $i = 1;   // line 0 is the request line, not a header
    while ($i < count($lines)) {
        $line = $lines[$i];
        $colon = str_index($line, ":");
        if ($colon >= 0) {
            if (header_name($line, $colon) === $want) {
                return header_value($line, $colon);
            }
        }
        $i = $i + 1;
    }
    return "";
}

// Lowercased header name from a "Name: Value" line, given the colon index.
function header_name(string $line, int $colon): string {
    return strtolower(trim(substr($line, 0, $colon)));
}

// Trimmed header value from a "Name: Value" line, given the colon index.
function header_value(string $line, int $colon): string {
    $after = intval($colon + 1);
    return trim(substr($line, $after));
}

// --- query string ----------------------------------------------------------

// Read a query-string parameter, e.g. query_param($req, "name", "World").
function query_param(Request $req, string $key, string $fallback): string {
    if (strlen($req->query) === 0) {
        return $fallback;
    }
    $pairs = explode("&", $req->query);
    $i = 0;
    while ($i < count($pairs)) {
        $pair = $pairs[$i];
        $eq = str_index($pair, "=");
        if ($eq >= 0) {
            if (pair_key($pair, $eq) === $key) {
                return pair_value($pair, $eq);
            }
        }
        $i = $i + 1;
    }
    return $fallback;
}

// Key half of a "key=value" pair, given the '=' index.
function pair_key(string $pair, int $eq): string {
    return substr($pair, 0, $eq);
}

// URL-decoded value half of a "key=value" pair, given the '=' index.
function pair_value(string $pair, int $eq): string {
    $after = intval($eq + 1);
    return urldecode(substr($pair, $after));
}
