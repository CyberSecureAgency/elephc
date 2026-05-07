---
title: "Fibers"
description: "Cooperative coroutines (PHP 8.1+ Fiber): create, start, suspend, resume, with FiberError."
sidebar:
  order: 12
---

A `Fiber` is a cooperative coroutine: a callable that owns its own call stack and can suspend execution at arbitrary depth. Control alternates explicitly between the caller and the fiber — there is no preemption.

## Class summary

| Method | Signature | Behavior |
|---|---|---|
| `__construct` | `__construct(callable $callback)` | Allocate a fiber and capture the callable. The callable runs the first time `start()` is called. |
| `start` | `start(mixed $arg0 = null, ..., mixed $arg6 = null): mixed` | Switch into the fiber and run it until it suspends or returns. Up to seven Mixed arguments are forwarded to the closure (it must declare its parameters as `mixed` to receive them). Returns the value the fiber yielded via `Fiber::suspend()` (or its terminal return). |
| `resume` | `resume(mixed $value = null): mixed` | Deliver a value to the fiber's pending `Fiber::suspend()` call and continue execution. Returns the next value the fiber yields. |
| `throw` | `throw(Throwable $exception): mixed` | Re-raise an exception inside the fiber at its pending suspend point. The exception unwinds the fiber's local `try`/`catch` chain — see *Limitations* below for the current escape behavior. |
| `getReturn` | `getReturn(): mixed` | Read the value the fiber returned after termination. Behavior is undefined if called before the fiber terminates. |
| `isStarted` | `isStarted(): bool` | True once `start()` has been called. |
| `isSuspended` | `isSuspended(): bool` | True while the fiber is paused at a `Fiber::suspend()` call. |
| `isRunning` | `isRunning(): bool` | True while the fiber is currently executing. |
| `isTerminated` | `isTerminated(): bool` | True after the fiber's callable has returned. |
| `Fiber::suspend` | `static suspend(mixed $value = null): mixed` | (Called from inside a fiber.) Yield the value to the resumer and pause; resumes with the value the next `resume()` delivers. |
| `Fiber::getCurrent` | `static getCurrent(): mixed` | The currently executing fiber, or null when called from the main thread. |

`FiberError` is a regular `Exception` subclass — `catch (Exception $e)` and `catch (FiberError $e)` both apply.

## Lifecycle states

A fiber moves through four states in order, never going backwards:

| State | When |
|---|---|
| `NotStarted` | Just constructed; `start()` has not been called yet. |
| `Running` | Currently executing. `Fiber::getCurrent()` returns this fiber. |
| `Suspended` | Paused inside `Fiber::suspend()`, waiting for a `resume()` from the caller. |
| `Terminated` | The callable has returned. `start()`/`resume()` are no longer valid on it. |

## Example

```php
<?php
$counter = new Fiber(function(): void {
    $a = Fiber::suspend(1);
    $b = Fiber::suspend($a + 10);
    Fiber::suspend($b + 100);
});

echo $counter->start();    // 1
echo " ";
echo $counter->resume(2);  // 12
echo " ";
echo $counter->resume(3);  // 103
```

Output:
```
1 12 103
```

## Implementation notes & known limitations

The current implementation targets ARM64 (macOS / Linux). The fiber stack is allocated from the standard heap, the context switch saves the AArch64 callee-saved registers (`x19-x28`, `x29-x30`, `d8-d15`), and the global exception handler chain head is swapped at every switch so a fiber's `try`/`catch` frames do not leak into the caller.

| Limitation | Notes |
|---|---|
| Mixed payloads round-trip but elephc's auto-unboxing for arithmetic is incomplete | `start()`, `resume()`, `Fiber::suspend()`, `getReturn()` all transmit Mixed cells, so `int`, `string`, and other scalars cross the suspend boundary cleanly when they are echoed or compared as Mixed. Arithmetic such as `$a + 10` on a Mixed value received from `Fiber::suspend()` does not auto-unbox today and yields the underlying cell pointer arithmetic — cast through a plain `int($a)` first if you need to compute on the value. |
| `start()` is fixed-arity (≤ 7 args), not truly variadic | The signature is seven optional Mixed parameters with `null` defaults — that exhausts the AArch64 integer arg-reg budget after `$this`. Calls with more than seven arguments are a type-check error. Closure parameters must be declared `mixed` to receive Mixed payloads — without an explicit type, params default to `int` and the closure sees the cell pointer as a raw integer. The same applies to the closure's return type: declare `: mixed` if you want `$f->start()` / `$f->resume()` to return the closure's terminal value cleanly. A closure declared `: int` returning a raw int into the Mixed return slot crashes when main code later dereferences it as a Mixed cell pointer. |
| ~~Closures with `use(...)` captures~~ | Supported for all scalar and reference types. When the user writes `new Fiber(function(...) use ($a, $b) { ... })`, the codegen evaluates each captured variable in the surrounding scope at construction time and stashes the value into the Fiber's slot files. A `user_arg_max` field on the Fiber tells `start()` to leave the captured int slots untouched. Each int-class capture is incref'd at construction and decref'd when the Fiber is freed, so heap-backed captures (objects, arrays, persisted strings) survive the original variable being reassigned or going out of scope. Int-class captures (int, bool, object pointer, callable, mixed) ride in `start_args` (one slot each); string captures consume two consecutive `start_args` slots (pointer + length); float captures ride in a parallel `float_args` file that the trampoline loads into `d0..d6`. The closure ABI numbers integer and float parameters independently, so the int-slot budget is 7 and the float-slot budget is also 7. Excess captures of either kind are dropped with a codegen warning. |
| ~~`Fiber->throw()` does not propagate uncaught exceptions~~ | Fixed. The trampoline installs a sentinel exception handler at the bottom of every fiber's chain. When an exception unwinds past every user `catch`, it is parked on the fiber, the fiber is marked Terminated, and control returns to the caller; the caller-side `start`/`resume`/`throw` helper then re-raises it on the caller's stack so a surrounding `try`/`catch` catches it. |
| ~~No `FiberError` raised on invalid state transitions~~ | Fixed. `start()` on a non-NotStarted fiber, `resume()`/`throw()` on a non-Suspended fiber, `getReturn()` before termination, and `Fiber::suspend()` outside a fiber now all raise `FiberError` with a PHP-equivalent message. |
| No `FiberError` on invalid state transitions | `resume()` on a fresh fiber, `getReturn()` before termination, or `Fiber::suspend()` outside a fiber all silently degrade to a no-op or `0` for the moment. |
| ~~No guard page on the fiber stack~~ | Fixed. Each fiber stack is allocated via `mmap` with `MAP_PRIVATE \| MAP_ANON`; the bottom 16 KB is `mprotect(PROT_NONE)` so a stack overflow faults via SIGSEGV/SIGBUS instead of silently corrupting the heap. The full mmap region is returned to the OS via `munmap` when the Fiber object's refcount drops to zero. |
| x86_64 not yet implemented | The fiber runtime is ARM64-only; the x86_64 build emits stub routines. |
