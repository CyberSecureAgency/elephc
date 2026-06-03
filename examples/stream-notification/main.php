<?php

// Stream-context notification callbacks.
//
// A `notification` callback registered on a stream context is fired during
// `http://` transfers at the STREAM_NOTIFY_* milestones: CONNECT once the TCP
// connection is up, COMPLETED once the whole body is buffered (with the byte
// count), and FAILURE when the open fails.
//
// This example points at a closed port (127.0.0.1:1) so it is fully
// self-contained — the connection is refused and the callback fires
// STREAM_NOTIFY_FAILURE. Point `fopen` at a real URL to see CONNECT + COMPLETED.

$ctx = stream_context_create([], [
    'notification' => function (
        int $code,
        int $severity,
        ?string $message,
        int $message_code,
        int $bytes_transferred,
        int $bytes_max
    ): void {
        if ($code === STREAM_NOTIFY_CONNECT) {
            echo "connected\n";
        } elseif ($code === STREAM_NOTIFY_COMPLETED) {
            echo "completed: " . $bytes_transferred . " bytes\n";
        } elseif ($code === STREAM_NOTIFY_FAILURE) {
            echo "failed (severity " . $severity . ")\n";
        }
    },
]);

$stream = fopen('http://127.0.0.1:1/', 'r');

echo $stream === false ? "open returned false\n" : "open succeeded\n";
