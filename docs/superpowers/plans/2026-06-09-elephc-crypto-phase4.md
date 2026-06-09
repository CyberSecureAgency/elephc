# elephc-crypto Phase 4 (incremental HashContext) Plan

> Driven directly in the main thread (per the user's preference), not via implementation subagents.

**Goal:** Implement PHP's incremental hashing — `hash_init`, `hash_update`, `hash_final`, `hash_copy` — on the `elephc-crypto` crate, modeling `HashContext` as a `fopen`-style resource handle.

**Spec:** `docs/superpowers/specs/2026-06-09-elephc-crypto-design.md` (Phase 4), with two approved deviations recorded below.

## Decisions / deviations from the original spec
1. **No scope auto-free** (user decision): a `HashContext` that leaves scope without `hash_final()` is NOT auto-freed — identical to how elephc treats an unclosed `fopen()` stream today (it leaks the fd until process exit; `functions/cleanup.rs` never cleans `Resource`s). Documented as a limitation shared with stream resources. General Resource scope-cleanup is a separate future enhancement. `elephc_crypto_free` therefore isn't wired (nothing calls it); `hash_final` frees via `elephc_crypto_final`.
2. **`hash_init` HASH_HMAC mode deferred**: `HASH_HMAC` is not a defined constant and adding it needs a new predefined-int-constant table (cf. `STREAM_INT_CONSTANTS`). For a rare feature, Phase 4 supports only plain `hash_init($algo)`; the checker errors on the flags/key form with "HASH_HMAC streaming mode is not supported; use hash_hmac()". The crate's `elephc_crypto_init_hmac` stays for a future addition.

## Resource model (mirror fopen/fclose)
- A resource value is a Mixed cell **tag 9** wrapping the handle (`fopen` boxes its fd via `__rt_mixed_from_value`; `__rt_hash_init`/`__rt_hash_copy` box the `elephc_crypto_init`/`_clone` handle the same way). Emit returns `PhpType::Mixed`; checker returns `PhpType::Mixed` (so the stored variable is unbox-compatible).
- Reading a resource arg: `crate::codegen::builtins::io::stream_arg::emit_stream_fd_arg` (pub(crate)) evaluates the variable and unboxes tag-9 → raw handle in `x0`/`rax`. Reused by `hash_update`/`hash_final`/`hash_copy`.

## New machinery
- Slots in `runtime/data/fixed.rs`: `_elephc_crypto_init_fn`, `_elephc_crypto_update_fn`, `_elephc_crypto_final_fn`, `_elephc_crypto_clone_fn`. Extend `publish_elephc_crypto_function_pointers` (in `builtins/strings/hash_crypto.rs`) to publish all four.
- Crate ABI (already built): `elephc_crypto_init(name,len)->*ctx (null=unknown)`, `elephc_crypto_update(ctx,data,len)`, `elephc_crypto_final(ctx,out)->isize`, `elephc_crypto_clone(ctx)->*ctx`.
- Runtime helpers (`runtime/strings/hash_context.rs`, both arches), reusing `__rt_digest_to_string`:
  - `__rt_hash_init`: algo (x1/x2) → C-ABI init via `_init_fn` (fail-closed); null handle → throw the hash `ValueError`; else box tag-9 → Mixed.
  - `__rt_hash_update`: handle + data → `elephc_crypto_update`; return PHP `true`.
  - `__rt_hash_final`: handle (+binary flag) → `elephc_crypto_final` into a 64-byte stack buffer → `__rt_digest_to_string` (hex/raw). Consumes the ctx.
  - `__rt_hash_copy`: handle → `elephc_crypto_clone` → box tag-9 → Mixed.

## Builtins (emitters in `builtins/strings/hash_context.rs`, dispatched in strings/mod.rs)
- `hash_init($algo)` → eval algo, publish, call `__rt_hash_init`; returns Mixed. Checker: exactly 1 arg (else the HMAC-mode error), `require_builtin_library("elephc_crypto")`, return Mixed.
- `hash_update($ctx,$data)` → `emit_stream_fd_arg(ctx)` → handle preserved; eval data; call `__rt_hash_update`; returns Bool.
- `hash_final($ctx,$binary=false)` → `emit_stream_fd_arg(ctx)` → handle; binary flag; call `__rt_hash_final`; returns Str.
- `hash_copy($ctx)` → `emit_stream_fd_arg(ctx)` → handle; publish; call `__rt_hash_copy`; returns Mixed.
- catalog: add all four. signatures: `hash_init`=optional(["algo","flags","key"],1,[int 0,""]) (checker enforces 1-arg-only for now); `hash_update`=fixed(["context","data"]); `hash_final`=optional(["context","binary"],1,[false]); `hash_copy`=fixed(["context"]). First-class sigs as appropriate. Effects: init/update/final/copy carry state → NOT pure (default impure; do not add to pure list).

## Tests (codegen, PHP-golden)
- `hash_init("sha256")`+update("ab")+update("c")+final == `ba7816bf...20015ad` (== sha256("abc")).
- binary: `bin2hex(hash_final(... ,true)) === hash_final(...)`.
- copy independence: init+update("a"), copy, update("bc")/update("XY") → finals `ba7816bf...` (abc) / `8411259f736c55dc19cfc1728693503c8e571d2d9ac272bb674636e956f2e49d` (aXY), and differ.
- hash_init unknown algo → `\ValueError` (PHP message).
- hash_init flags/HMAC-mode form → compile error (documented limitation).

## Gate
Per-builtin codegen tests → full `cargo test` → `--include-ignored` (only live-DB failures) → Docker Linux x86_64 + arm64 (`hash_init`/`hash_update`/`hash_final`/`hash_copy`) → `git diff --check` + warnings + asm-comment alignment. Docs/example/roadmap land in Phase 5.
