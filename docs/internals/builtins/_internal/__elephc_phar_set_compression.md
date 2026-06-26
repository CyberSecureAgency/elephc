---
title: "__elephc_phar_set_compression() — internals"
description: "Compiler internals for __elephc_phar_set_compression(): PHAR archive compression control for helper classes."
sidebar:
  order: 259
---

## `__elephc_phar_set_compression()` — internals

## Where it lives

- **Signature**: [`src/types/signatures.rs`](https://github.com/illegalstudio/elephc/blob/main/src/types/signatures.rs)
- **Type checking**: [`src/types/checker/builtins/io/files.rs`](https://github.com/illegalstudio/elephc/blob/main/src/types/checker/builtins/io/files.rs)
- **Lowering**: [`src/codegen_ir/lower_inst/builtins/io.rs`](https://github.com/illegalstudio/elephc/blob/main/src/codegen_ir/lower_inst/builtins/io.rs) (`lower_elephc_phar_set_compression`)
- **Bridge**: [`crates/elephc-phar/src/lib.rs`](https://github.com/illegalstudio/elephc/blob/main/crates/elephc-phar/src/lib.rs) (`elephc_phar_set_compression`)

### Lowering notes

- Internal helper used by the built-in `Phar` / `PharData` support to change archive compression.
- Requires the `elephc-phar` bridge and publishes the `elephc_phar_set_compression` function pointer before calling through the runtime slot.
- Returns `true` on bridge success and `false` when the archive or compression mode cannot be handled.

## Runtime helpers

The lowering publishes and calls:
- `_elephc_phar_set_compression_fn`
- `elephc_phar_set_compression`

## Signature summary

```php
function __elephc_phar_set_compression(string $filename, int $compression): bool
```

## What the type checker enforces

- **Arity**: takes exactly 2 arguments.
- **Bridge dependency**: requires `elephc_phar`.

## Cross-references

- _No user-facing reference — this is a compiler internal helper._
