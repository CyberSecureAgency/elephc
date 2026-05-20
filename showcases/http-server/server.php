<?php
// server.php — the event loop: a level-triggered poll() reactor that drives
// one Fiber per connection.
//
// The loop never blocks on a socket. poll() reports which descriptors are
// ready, and only then is accept()/read() called — so every syscall returns
// immediately. Each connection runs inside its own Fiber, which suspends
// whenever it needs more data and is resumed when poll() says so.
//
// Connections live in a fixed pool allocated once at startup; idle slots are
// reused, so steady-state traffic performs no per-request allocation.

// Raw-memory helpers for the pollfd array. The byte offset is taken as a
// parameter so the offset pointer is formed inside this one small function.
function poke32(ptr $p, int $off, int $value): void {
    $cell = ptr_offset($p, $off);
    ptr_write32($cell, $value);
}

function poke16(ptr $p, int $off, int $value): void {
    $cell = ptr_offset($p, $off);
    ptr_write16($cell, $value);
}

function peek16(ptr $p, int $off): int {
    $cell = ptr_offset($p, $off);
    return ptr_read16($cell);
}

// Block in poll() until a descriptor is ready.
function poll_wait(ptr $fds, int $nfds): int {
    return poll($fds, $nfds, -1);
}

// One pooled client connection and its in-progress request.
class Connection {
    public int $fd = 0;
    public $fiber = null;
    public string $inbuf = "";
    public bool $active = false;
    public bool $started = false;
    public bool $closed = false;

    // Reuse this idle slot for a freshly accepted connection.
    public function reset(int $fd): void {
        $this->fd = $fd;
        $this->fiber = null;
        $this->inbuf = "";
        $this->active = true;
        $this->started = false;
        $this->closed = false;
    }
}

// Build the Fiber that reads, parses, and answers a single connection. The
// Fiber suspends whenever the request is incomplete, handing control back to
// the event loop so other connections can make progress.
function make_conn_fiber(Connection $conn) {
    return new Fiber(function() use ($conn): void {
        $fd = $conn->fd;

        // Accumulate bytes until the full header block has arrived.
        while (strpos($conn->inbuf, "\r\n\r\n") === false) {
            $chunk = socket_recv($fd);
            if ($chunk === "") {
                // Peer closed before sending a complete request.
                close($fd);
                $conn->closed = true;
                return;
            }
            $conn->inbuf = $conn->inbuf . $chunk;
            if (strpos($conn->inbuf, "\r\n\r\n") === false) {
                Fiber::suspend(0);   // yield: wait for the rest of the request
            }
        }

        // Parse, route, and write the response.
        $req = parse_request($conn->inbuf);
        $res = handle_request($req);
        $payload = $res->render();
        socket_send($fd, $payload);
        echo "  " . $req->method . " " . $req->path . " -> " . $res->status . "\n";

        close($fd);
        $conn->closed = true;
    });
}

// Start a connection's Fiber on first contact, or resume it afterwards.
function service_connection(Connection $conn): void {
    if (!$conn->started) {
        $conn->started = true;
        $conn->fiber = make_conn_fiber($conn);
        $conn->fiber->start();
    } elseif (!$conn->fiber->isTerminated()) {
        $conn->fiber->resume(0);
    }
}

// Bind $port and run the poll()/Fiber event loop until the process is stopped.
function run_http_server(int $port): void {
    $max = 64;   // size of the connection pool

    $listen_fd = tcp_listen($port);
    if ($listen_fd < 0) {
        echo "error: could not bind port " . $port . "\n";
        return;
    }
    echo "elephc http-server listening on http://127.0.0.1:" . $port . "\n";
    echo "press Ctrl+C to stop\n";

    // Allocate the connection pool once; idle slots have active = false.
    $slots = [];
    $i = 0;
    while ($i < $max) {
        $slots[] = new Connection();
        $i = $i + 1;
    }

    // One `struct pollfd` (8 bytes) per connection, plus the listener.
    $poll_bytes = ($max + 1) * 8;
    $pollfds = malloc($poll_bytes);

    while (true) {
        // Poll slot 0 watches the listener; one slot follows per active
        // connection. $poll_map[p] is the pool index behind poll slot p+1.
        poke32($pollfds, 0, $listen_fd);
        poke16($pollfds, 4, 1);   // events = POLLIN
        poke16($pollfds, 6, 0);   // revents
        $poll_map = [];
        $active = 0;
        $i = 0;
        while ($i < $max) {
            if ($slots[$i]->active) {
                $base = ($active + 1) * 8;
                poke32($pollfds, $base, $slots[$i]->fd);
                poke16($pollfds, $base + 4, 1);
                poke16($pollfds, $base + 6, 0);
                $poll_map[] = $i;
                $active = $active + 1;
            }
            $i = $i + 1;
        }

        // Block until at least one descriptor becomes ready.
        $nfds = $active + 1;
        if (poll_wait($pollfds, $nfds) < 0) {
            break;
        }

        // Service every connection whose socket reported activity.
        $p = 0;
        while ($p < $active) {
            $rev_off = ($p + 1) * 8 + 6;
            if (peek16($pollfds, $rev_off) != 0) {
                service_connection($slots[$poll_map[$p]]);
            }
            $p = $p + 1;
        }

        // A ready listener means a new connection is waiting.
        if (peek16($pollfds, 6) != 0 && $active < $max) {
            $cfd = socket_accept($listen_fd);
            if ($cfd >= 0) {
                $free = -1;
                $i = 0;
                while ($i < $max) {
                    if (!$slots[$i]->active) {
                        $free = $i;
                        $i = $max;
                    } else {
                        $i = $i + 1;
                    }
                }
                if ($free >= 0) {
                    $slots[$free]->reset($cfd);
                } else {
                    close($cfd);
                }
            }
        }

        // Release the slots whose connection has finished.
        $i = 0;
        while ($i < $max) {
            if ($slots[$i]->active && $slots[$i]->closed) {
                $slots[$i]->active = false;
            }
            $i = $i + 1;
        }
    }

    close($listen_fd);
}
