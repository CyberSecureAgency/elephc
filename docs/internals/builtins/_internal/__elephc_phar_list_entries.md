---
title: "__elephc_phar_list_entries() — internals"
description: "Compiler internals for __elephc_phar_list_entries(): PHAR archive entry enumeration for helper classes."
sidebar:
  order: 258
---

## `__elephc_phar_list_entries()` — internals

## Where it lives

- **Signature**: [`src/types/signatures.rs`](https://github.com/illegalstudio/elephc/blob/main/src/types/signatures.rs)
- **Type checking**: [`src/types/checker/builtins/io/files.rs`](https://github.com/illegalstudio/elephc/blob/main/src/types/checker/builtins/io/files.rs)
- **Lowering**: [`src/codegen_ir/lower_inst/builtins/io.rs`](https://github.com/illegalstudio/elephc/blob/main/src/codegen_ir/lower_inst/builtins/io.rs) (`lower_elephc_phar_list_entries`)
- **Bridge**: [`crates/elephc-phar/src/lib.rs`](https://github.com/illegalstudio/elephc/blob/main/crates/elephc-phar/src/lib.rs) (`elephc_phar_list_entries`)

### Lowering notes

- Internal helper used by the built-in `Phar` / `PharData` support to enumerate archive entries.
- Requires the `elephc-phar` bridge and publishes the `elephc_phar_list_entries` function pointer before calling through the runtime slot.
- Returns an indexed array of entry-name strings.

## Runtime helpers

The lowering publishes and calls:
- `_elephc_phar_list_entries_fn`
- `elephc_phar_list_entries`

## Signature summary

```php
function __elephc_phar_list_entries(string $filename): array
```

## What the type checker enforces

- **Arity**: takes exactly 1 argument.
- **Bridge dependency**: requires `elephc_phar`.

## Cross-references

- _No user-facing reference — this is a compiler internal helper._
