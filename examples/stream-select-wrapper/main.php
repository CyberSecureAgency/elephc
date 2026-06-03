<?php

// Selecting a userspace-wrapper stream via stream_cast().
//
// A synthetic wrapper file descriptor cannot be handed to select(2) directly.
// When a wrapper class implements stream_cast(), stream_select() resolves the
// wrapper stream to its real underlying fd (STREAM_CAST_FOR_SELECT) and polls
// that — so a wrapper that fronts a real socket becomes select()-able.
//
// Here the wrapper connects to a same-process server inside stream_open() and
// returns that connection from stream_cast(). The server writes a byte, which
// makes the wrapper stream readable.

class SocketWrapper
{
    public $context;
    public $inner;

    public function stream_open($path, $mode, $options, &$opened): bool
    {
        $this->inner = stream_socket_client("tcp://127.0.0.1:55190");
        return $this->inner !== false;
    }

    // Expose the real underlying socket fd so stream_select() can poll it.
    public function stream_cast($cast_as)
    {
        return $this->inner;
    }

    public function stream_eof(): bool
    {
        return false;
    }

    public function stream_read(int $count): string
    {
        return "";
    }
}

stream_wrapper_register("sock", "SocketWrapper");

$server = stream_socket_server("tcp://127.0.0.1:55190");
$stream = fopen("sock://connection", "r");   // connects to the server inside stream_open()
$peer = stream_socket_accept($server);
fwrite($peer, "hello");                       // make the wrapper stream readable

$read = [$stream];
$write = [];
$except = [];
$ready = stream_select($read, $write, $except, 1, 0);

echo "ready streams: " . $ready . "\n";
echo "wrapper selected: " . (count($read) === 1 ? "yes" : "no") . "\n";
