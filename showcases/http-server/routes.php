<?php
// routes.php — the application: route handlers and the request dispatcher.
//
// elephc compiles handler closures stored in a lookup table into uncallable
// values, so routing here is an explicit dispatch to named handler functions.
// Each handler takes the Request and returns a Response.

// GET / — landing page.
function route_index(): Response {
    $res = new Response();
    $res->html(
        "<!doctype html>\n"
        . "<html><head><title>elephc http-server</title></head><body>\n"
        . "<h1>elephc http-server</h1>\n"
        . "<p>A native HTTP/1.1 server written in PHP and compiled to a\n"
        . "standalone binary by elephc &mdash; no interpreter, no VM,\n"
        . "no PHP-FPM, no Nginx.</p>\n"
        . "<ul>\n"
        . "<li><a href=\"/hello?name=elephc\">/hello?name=elephc</a></li>\n"
        . "<li><a href=\"/json\">/json</a></li>\n"
        . "<li><a href=\"/stats\">/stats</a></li>\n"
        . "</ul>\n"
        . "</body></html>\n"
    );
    return $res;
}

// GET /hello — greeting that reads the ?name= query parameter.
function route_hello(Request $req): Response {
    $name = query_param($req, "name", "World");
    $res = new Response();
    $res->text("Hello, " . $name . "!\n");
    return $res;
}

// GET /json — a small JSON document built with json_encode().
function route_json(Request $req): Response {
    $payload = [
        "server"   => "elephc-http",
        "language" => "PHP compiled to native code",
        "os"       => PHP_OS,
        "path"     => $req->path,
    ];
    $res = new Response();
    $res->json(json_encode($payload));
    return $res;
}

// GET /stats — server and request information.
function route_stats(Request $req, int $served): Response {
    $res = new Response();
    $res->text(
        "elephc http-server\n"
        . "os:               " . PHP_OS . "\n"
        . "requests served:  " . $served . "\n"
        . "your user-agent:  " . $req->header("User-Agent") . "\n"
    );
    return $res;
}

// Fallback for any unknown path.
function route_not_found(Request $req): Response {
    $res = new Response();
    $res->status = 404;
    $res->text("404 Not Found: " . $req->path . "\n");
    return $res;
}

// Dispatch a parsed request to a handler and return its response.
// A static counter records how many requests have been served so far.
function handle_request(Request $req): Response {
    static $served = 0;
    $served = $served + 1;

    if ($req->method !== "GET") {
        $res = new Response();
        $res->status = 405;
        $res->text("405 Method Not Allowed\n");
        return $res;
    }

    if ($req->path === "/") {
        return route_index();
    } elseif ($req->path === "/hello") {
        return route_hello($req);
    } elseif ($req->path === "/json") {
        return route_json($req);
    } elseif ($req->path === "/stats") {
        return route_stats($req, $served);
    }
    return route_not_found($req);
}
