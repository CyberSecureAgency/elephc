---
title: "FFI & Extern"
description: "Foreign Function Interface: calling C libraries, extern functions, globals, classes, and callbacks."
sidebar:
  order: 4
---

FFI lets elephc programs call C library functions directly, with automatic type marshalling.

## Declaring extern functions
```php
<?php
extern function abs(int $n): int;
extern function getpid(): int;

// With explicit library
extern "curl" function curl_easy_init(): ptr;

// Block syntax
extern "SDL2" {
    function SDL_Init(int $flags): int;
    function SDL_Quit(): void;
}
```

## Supported C types

| elephc type | C equivalent | Passed in |
|---|---|---|
| `int` | `int64_t` / `long` | integer argument register |
| `float` | `double` | floating-point argument register |
| `bool` | `int` (0/1) | integer argument register |
| `string` | `char*` (auto null-terminated) | integer argument register |
| `ptr` | `void*` | integer argument register |
| `ptr<T>` | `T*` | integer argument register |
| `void` | void (return only) | — |
| `callable` | function pointer | integer argument register |

Argument registers follow the selected target's C ABI: AArch64 uses
`x0`-`x7` for integers/pointers and `d0`-`d7` for doubles, while Linux
x86_64 uses the System V registers (`rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`
for integers/pointers and `xmm0`-`xmm7` for doubles).

`callable` in an `extern function` signature is a special FFI-only path: the
call site must provide a string-literal elephc function name, and codegen passes
that function's native symbol address to C. General PHP callable values use
runtime descriptor pointers internally and are not passed directly to C as
closures or first-class callable descriptors.

## String conversion
- **Calling C**: elephc creates temporary null-terminated copy, frees after call
- **C returns string**: elephc scans for `\0`, copies to owned storage

Extern calls support the declared parameter names:

```php
<?php
extern function strcmp(string $left, string $right): int;

$left = ["a"];
echo strcmp(...$left, right: "b");
```

Argument expressions are evaluated in PHP source order, then elephc loads the
resulting values into the target C ABI registers. This matters when positional,
named, or spread arguments have side effects.

Declared extern functions can also be selected by dynamic PHP string callbacks
such as `call_user_func($cb, ...)` and `call_user_func_array($cb, $args)`.
The runtime descriptor uses an extern invocation shape and a generated PHP-ABI
wrapper; the wrapper performs the C ABI call after PHP argument normalization.

## Callbacks
```php
<?php
extern function signal(int $sig, callable $handler): ptr;

function on_signal($sig) {
    echo "caught signal " . $sig . "\n";
}

signal(15, "on_signal");
```
Callbacks must use C-compatible types only. No strings, arrays, variadic, defaults, or pass-by-reference.

## Extern globals
```php
<?php
extern global ptr $environ;
```
Uses GOT-relative addressing. String globals auto-converted.

## Extern classes (C structs)
```php
<?php
extern class Point {
    public int $x;
    public int $y;
}

$p = ptr_cast<Point>(malloc(16));
$p->x = 10;
echo $p->x;   // 10
```
Flat sequential layout, no class_id/vtable. 8-byte alignment.

## CLI linker flags

| Flag | Description |
|---|---|
| `--link LIB` / `-lLIB` | Link additional library |
| `--link-path DIR` / `-LDIR` | Add library search path |
| `--framework NAME` | Link macOS framework |

Libraries in `extern "lib" {}` blocks are linked automatically.
