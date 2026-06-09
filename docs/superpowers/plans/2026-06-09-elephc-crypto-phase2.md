# elephc-crypto Phase 2 (migrate one-shot hash/md5/sha1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Route `hash()`, `md5()`, `sha1()` through the `elephc-crypto` staticlib instead of CommonCrypto/libcrypto — unlocking the full PHP algorithm set, the `$binary` raw-output flag, and a real `ValueError` on an unknown algorithm — while keeping byte-for-byte parity for existing output. `crc32()` is untouched (pure asm, stays). phar's CC_SHA1 and all CommonCrypto/libcrypto machinery stay until Phase 5.

**Architecture:** Mirror the elephc-tls slot model. A BSS slot `_elephc_crypto_hash_fn` holds the address of `elephc_crypto_hash`, published at each hash/md5/sha1 call site (so non-hashing programs never link the crate). The runtime routines marshal (algo, data) into the C ABI, call the slot fail-closed, raise `ValueError` on a `-1` return, then format the raw digest as lowercase hex — or return raw bytes when `$binary` is true.

**Tech Stack:** Rust, target-aware ARM64 + x86_64 assembly emitters.

**Spec:** `docs/superpowers/specs/2026-06-09-elephc-crypto-design.md` (Components 2 and 3; Phase 2). Phase 1 (the crate + linker bridge) is already merged on this branch.

---

## Key facts established by codebase mapping (do not re-derive)

- **Crate ABI (already built, Phase 1):** `elephc_crypto_hash(name_ptr, name_len, data_ptr, data_len, out_ptr) -> isize` writes the raw digest into a caller 64-byte buffer and returns the digest length, or `-1` for an unknown algorithm. (`crates/elephc-crypto/src/lib.rs`.)
- **Linker bridge (Phase 1):** linking is requested by `checker.require_builtin_library("elephc_crypto")` (ALL targets — not the linux-only variant). The string `"elephc_crypto"` matches the `BRIDGES` entry.
- **Slot pattern:** slots are emitted in `src/codegen/runtime/data/fixed.rs` via `out.push_str(".comm _name_fn, 8, 3\n");` (8 bytes, 8-byte aligned).
- **Publication pattern:** `publish_tls_function_pointers` in `src/codegen/builtins/io/https_stream.rs` is the template. Per entry it does: `abi::emit_extern_symbol_address(emitter, reg, &emitter.target.extern_symbol(c_name))` then `abi::emit_symbol_address(emitter, reg2, slot)` then store reg→[reg2] (ARM64 `str x9,[x10]`; x86_64 `mov QWORD PTR [rip+slot], r9`). It's called at the call site that uses the runtime routine.
- **Fail-closed indirection:** load slot (`abi::emit_symbol_address` + `ldr`/`mov`), `cbz`/`test`→fail label, then indirect call (`abi::emit_call_reg`, i.e. `blr`/`call`). See `src/codegen/runtime/io/https.rs`.
- **abi:: helpers:** `emit_symbol_address(emitter,reg,sym)` (BSS addr), `emit_extern_symbol_address(emitter,reg,sym)` (extern via GOT), `emit_call_reg(emitter,reg)` (indirect call), `emit_call_label(emitter,label)` (direct call), `emit_push_reg_pair`/`emit_pop_reg_pair`. `emitter.target.extern_symbol(name)` produces the platform-correct extern symbol name.
- **Existing runtime routines** are emitted UNCONDITIONALLY in `src/codegen/runtime/emitters.rs` (≈lines 92-95: `strings::emit_md5/emit_sha1/emit_crc32/emit_hash`). They reference only the BSS slot (always defined), never `elephc_crypto_hash` directly — so always-emitting them does not force the crate link. Only the call-site `publish` references `elephc_crypto_hash`.
- **Current builtin emitters:** `src/codegen/builtins/strings/{hash,md5,sha1,crc32}.rs`; dispatcher `src/codegen/builtins/strings/mod.rs`. `hash::emit` evaluates algo into the string regs then data into the secondary regs (ARM64 algo `x1/x2`, data `x3/x4`; x86_64 algo `rax/rdx`, data `rdi/rsi`) and calls `__rt_hash`. `md5`/`sha1` use `super::args::emit_string_arg(&args[0],…)` and call `__rt_md5`/`__rt_sha1`, **ignoring their `$binary` second arg**.
- **Current runtime hash routines** (`src/codegen/runtime/strings/{hash,md5,sha1}.rs`) call `CC_MD5`/`CC_SHA1`/`CC_SHA256` (10 call sites) and hex-format inline (nibble loop writing to `_concat_buf`/`_concat_off`). These get rewritten.
- **ValueError throw from runtime (template):** `src/codegen/builtins/math/clamp.rs` `emit_throw_value_error_aarch64`/`_x86_64`: `__rt_heap_alloc(32)` → stamp heap-kind object → store `_spl_value_error_class_id` at [obj], message ptr at [obj+8], len at [obj+16], 0 at [obj+24] → store obj into `_exc_value` → `b`/`jmp __rt_throw_current`. Catchable.
- **Checker sites to swap (Phase 2):** `src/types/checker/builtins/strings.rs:308` (hash) and `:328` (md5|sha1) currently call `require_linux_builtin_library("crypto")`. Change to `require_builtin_library("elephc_crypto")`. Leave `crc32` (no lib) and the phar sites (`io/streams.rs:71`, `io/files.rs:68`) untouched (Phase 5).
- **Signature fix:** `src/types/signatures.rs:236` `"hash" => Some(fixed(&["algo","data"]))` → `Some(optional(&["algo","data","binary"], 2, vec![bool_lit(false)]))`. md5/sha1 already have `binary`.
- **PHP parity facts (oracle = `php`):** unknown-algo `ValueError` message is exactly `hash(): Argument #1 ($algo) must be a valid hashing algorithm`. `bin2hex(md5($s,true)) === md5($s)`. `hash()`/`HASH()`/`\hash()` are equivalent (case-insensitive; the catalog already drives this).
- **NOT in Phase 2 (stays for Phase 5):** the `.weak MD5/SHA1/SHA256` decls (`emitters.rs:476-483`), the `CC_*→*` platform transform (`platform/linux_transform.rs`, `target.rs`), and phar's 2 `CC_SHA1` calls + 2 phar checker `require_linux_builtin_library("crypto")` sites. Do NOT remove these in Phase 2 — phar-write still uses them.

## Assembly conventions (MANDATORY for every touched runtime/emitter file)

- Every `emitter.instruction(...)` gets an inline `//` comment, `//` aligned to column 81 (pad with spaces; if code ≥ 80 chars, exactly one space before `//`). Use `// -- block --` headers before instruction groups.
- Use `abi::` helpers for symbol addresses / calls / stack — never hardcode per-arch register/stack mechanics in shared lowering.
- Both ARM64 and x86_64 paths must be implemented in the same change. macOS arm64 is covered by local `cargo test`; Linux x86_64/arm64 by the Docker scripts (Task 3).

---

## File Structure

- Modify: `src/codegen/runtime/data/fixed.rs` — add the `_elephc_crypto_hash_fn` slot.
- Create: `src/codegen/builtins/strings/hash_crypto.rs` — `publish_elephc_crypto_function_pointers(emitter)` (publishes `elephc_crypto_hash` into the slot) + a `throw-ValueError` helper for the unknown-algo path (or reuse clamp's pattern via a shared helper — see Task 1).
- Modify: `src/codegen/builtins/strings/mod.rs` — declare `mod hash_crypto;` and wire it.
- Rewrite: `src/codegen/runtime/strings/hash.rs` — `__rt_hash` now marshals to the slot, raises ValueError on -1, formats hex/raw.
- Rewrite: `src/codegen/runtime/strings/md5.rs`, `sha1.rs` — route through the shared digest path with a fixed algo, honor `$binary`.
- Modify: `src/codegen/builtins/strings/{hash,md5,sha1}.rs` — evaluate `$binary`, publish the pointer, pass the binary flag.
- Modify: `src/types/checker/builtins/strings.rs` — swap the two `require_*` calls.
- Modify: `src/types/signatures.rs` — add `binary` to `hash`.
- Test: `tests/codegen/` — new hashing codegen tests (find the existing md5/sha1 codegen tests and add alongside, or create `tests/codegen/hashing.rs` wired into the codegen test module).

---

### Task 1: Migrate `hash()` to the crate (slot + publish + runtime + $binary + ValueError + link)

**Files:** `runtime/data/fixed.rs`, `builtins/strings/hash_crypto.rs` (new), `builtins/strings/mod.rs`, `runtime/strings/hash.rs`, `builtins/strings/hash.rs`, `types/checker/builtins/strings.rs`, `types/signatures.rs`, plus codegen tests.

- [ ] **Step 1: Write failing codegen tests.**

First locate where existing hash/md5 codegen tests live: `rg -n "md5\(|__rt_md5|compile_and_run.*md5|hash\(" tests/`. Add the tests in that file (or create `tests/codegen/hashing.rs` and `mod hashing;` in the codegen test module — follow how sibling `tests/codegen/*.rs` are registered). Use the `compile_and_run` helper.

```rust
#[test]
fn hash_supports_full_algorithm_set() {
    assert_eq!(compile_and_run(r#"<?php echo hash("sha256","hello");"#),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    assert_eq!(compile_and_run(r#"<?php echo hash("sha512","hello");"#),
        "9b71d224bd62f3785d96d46ad3ea3d73319bfbc2890caadae2dff72519673ca72323c3d99ba5c11d7c7acc6e14b8c5da0c4663475c2e5c3adef46f73bcdec043");
    assert_eq!(compile_and_run(r#"<?php echo hash("sha3-256","hello");"#),
        "3338be694f50c5f338814986cdf0686453a888b84f424d792af4b9202398f392");
    assert_eq!(compile_and_run(r#"<?php echo hash("ripemd160","hello");"#),
        "108f07b8382412612c048d07d13f814118445acd");
    assert_eq!(compile_and_run(r#"<?php echo hash("crc32b","hello");"#), "3610a686");
}

#[test]
fn hash_md5_sha256_parity_regression() {
    assert_eq!(compile_and_run(r#"<?php echo hash("md5","abc");"#),
        "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(compile_and_run(r#"<?php echo hash("sha256","abc");"#),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[test]
fn hash_binary_flag_returns_raw_bytes() {
    // bin2hex(hash(algo, data, true)) === hash(algo, data)
    assert_eq!(compile_and_run(r#"<?php echo bin2hex(hash("sha256","abc",true));"#),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(compile_and_run(r#"<?php echo strlen(hash("sha256","abc",true));"#), "32");
}

#[test]
fn hash_unknown_algorithm_throws_value_error() {
    assert_eq!(
        compile_and_run(r#"<?php try { hash("nope","x"); } catch (\ValueError $e) { echo $e->getMessage(); }"#),
        "hash(): Argument #1 ($algo) must be a valid hashing algorithm"
    );
}

#[test]
fn hash_is_case_insensitive_and_namespaced() {
    assert_eq!(compile_and_run(r#"<?php echo HASH("md5","abc");"#),
        "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(compile_and_run(r#"<?php echo \hash("md5","abc");"#),
        "900150983cd24fb0d6963f7d28e17f72");
}
```

- [ ] **Step 2: Run them to confirm failure.**

Run: `cargo test --test codegen_tests hash_ 2>&1 | tail -20` (or the binary your codegen tests live in). Expected: the full-set / binary / ValueError / case tests FAIL (today `hash` only does md5/sha1/sha256, ignores `$binary`, returns "" for unknown, and isn't linked to the crate). The two parity assertions in `hash_md5_sha256_parity_regression` may already pass — that's fine.

- [ ] **Step 3: Add the BSS slot.**

In `src/codegen/runtime/data/fixed.rs`, next to the `_elephc_tls_*_fn` slots, add:
```rust
out.push_str(".comm _elephc_crypto_hash_fn, 8, 3\n");                       // elephc-crypto one-shot hash entry
```

- [ ] **Step 4: Create the publication + ValueError helper.**

Create `src/codegen/builtins/strings/hash_crypto.rs` with a module preamble. Implement `pub(crate) fn publish_elephc_crypto_function_pointers(emitter: &mut Emitter)` mirroring `publish_tls_function_pointers` but for the single entry `("elephc_crypto_hash", "_elephc_crypto_hash_fn")` (both ARM64 and x86_64 arms). Also add `pub(crate) fn emit_throw_unknown_algorithm(emitter, data)` (or pass the message label in) that throws `ValueError` with message `hash(): Argument #1 ($algo) must be a valid hashing algorithm` using the exact pattern from `src/codegen/builtins/math/clamp.rs` (`emit_throw_value_error_aarch64`/`_x86_64`): add the message via `data.add_string(...)`, then emit the alloc/stamp/`_spl_value_error_class_id`/`_exc_value`/`__rt_throw_current` sequence for both arches. (If cleaner, factor clamp's two functions into a shared `src/codegen/.../value_error.rs` helper that takes `(emitter, message_symbol, message_len)` and call it from both clamp and here — only do this if it does not change clamp's emitted output; verify clamp tests still pass.)

Declare `mod hash_crypto;` in `src/codegen/builtins/strings/mod.rs`.

- [ ] **Step 5: Rewrite `__rt_hash` (runtime) to use the slot.**

Rewrite `emit_hash` in `src/codegen/runtime/strings/hash.rs` for BOTH arches. Contract — on entry: algo ptr/len in the runtime's algo regs, data ptr/len in the data regs, and a **binary flag** in a dedicated register (define it: ARM64 `x5`, x86_64 `r10` — the builtin emitter in Step 6 sets it). Steps:
  1. Reserve a 64-byte raw-digest buffer on the stack (respect 16-byte alignment).
  2. Marshal into the C ABI for `elephc_crypto_hash(name_ptr, name_len, data_ptr, data_len, out_ptr)`: ARM64 `x0=algo_ptr,x1=algo_len,x2=data_ptr,x3=data_len,x4=out_buf`; x86_64 `rdi=algo_ptr,rsi=algo_len,rdx=data_ptr,rcx=data_len,r8=out_buf`. Preserve the binary flag across the call (stack or a callee-saved reg).
  3. Load `_elephc_crypto_hash_fn`, `cbz`/`test`→ a fail path. (Null slot ≈ crate not linked; route it to the same ValueError/throw — it cannot happen for a correctly-linked hashing program, but must fail closed, not call null.)
  4. Indirect-call the slot (`abi::emit_call_reg`). Result = digest length in `x0`/`rax`, or `-1`.
  5. If result `< 0`: throw the unknown-algorithm `ValueError` (call the helper from Step 4 / jump to its label).
  6. Else format the result into a PHP string in `_concat_buf` (advancing `_concat_off`): if the binary flag is 0, run the existing nibble→hex loop but **length-driven** by the returned digest length (not a fixed 16/20/32); if the binary flag is non-zero, copy the raw `length` bytes verbatim. Return ptr/len in the string result regs.

Factor the "raw buffer → `_concat_buf` (hex or raw)" formatting into a shared local routine (e.g. `__rt_digest_to_string`) so md5/sha1 reuse it in Task 2. Remove the old `CC_MD5`/`CC_SHA1`/`CC_SHA256` dispatch from this file.

- [ ] **Step 6: Update the `hash()` builtin emitter.**

In `src/codegen/builtins/strings/hash.rs`: after evaluating algo + data (keep the existing evaluation into algo/data regs), additionally evaluate the optional `$binary` arg (default `false` when absent) into the binary flag register (ARM64 `x5`, x86_64 `r10`) as a 0/1 integer — coerce via the existing bool/int coercion path. Call `hash_crypto::publish_elephc_crypto_function_pointers(emitter)` before the `__rt_hash` call so the slot is populated and the crate symbol is referenced. Keep `Some(PhpType::Str)`.

- [ ] **Step 7: Swap the conditional link + fix the signature.**

- In `src/types/checker/builtins/strings.rs`, the `"hash"` arm (≈line 308): replace `checker.require_linux_builtin_library("crypto");` with `checker.require_builtin_library("elephc_crypto");`. Also update the arity check if needed so a 3rd `$binary` arg is accepted (it currently hard-requires `args.len() != 2`; change to allow 2 or 3, matching the new signature — e.g. `if args.len() < 2 || args.len() > 3`).
- In `src/types/signatures.rs` (≈line 236): `"hash" => Some(optional(&["algo", "data", "binary"], 2, vec![bool_lit(false)])),`.

- [ ] **Step 8: Build clean and run the tests.**

Run: `cargo build 2>&1 | tail -5` (zero warnings), then `cargo test --test codegen_tests hash_ 2>&1 | tail -25`. Expected: all five hash tests PASS. If a digest mismatches, the bug is in the marshalling/formatting (do not change the PHP-golden expected values). If the ValueError test hangs or crashes instead of printing the message, the throw sequence is wrong — compare against clamp.rs exactly.

- [ ] **Step 9: Verify assembly-comment alignment for touched runtime files.**

Run the column-81 checker from CLAUDE.md against `src/codegen/runtime/strings/hash.rs` and `src/codegen/builtins/strings/hash_crypto.rs`. Fix any misaligned `//`.

- [ ] **Step 10: Commit.**

```bash
git add src/codegen/runtime/data/fixed.rs src/codegen/builtins/strings/hash_crypto.rs src/codegen/builtins/strings/mod.rs src/codegen/runtime/strings/hash.rs src/codegen/builtins/strings/hash.rs src/types/checker/builtins/strings.rs src/types/signatures.rs tests/
git commit -m "feat(crypto): route hash() through elephc-crypto (full algo set, \$binary, ValueError)"
```

---

### Task 2: Migrate `md5()` and `sha1()` to the crate (parity + $binary)

**Files:** `runtime/strings/md5.rs`, `runtime/strings/sha1.rs`, `builtins/strings/md5.rs`, `builtins/strings/sha1.rs`, `types/checker/builtins/strings.rs`, codegen tests.

- [ ] **Step 1: Write failing codegen tests.**

```rust
#[test]
fn md5_sha1_parity_and_binary() {
    assert_eq!(compile_and_run(r#"<?php echo md5("abc");"#), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(compile_and_run(r#"<?php echo sha1("abc");"#), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(compile_and_run(r#"<?php echo md5("");"#), "d41d8cd98f00b204e9800998ecf8427e");
    // $binary raw output (was silently ignored before)
    assert_eq!(compile_and_run(r#"<?php echo bin2hex(md5("abc",true));"#), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(compile_and_run(r#"<?php echo strlen(md5("abc",true));"#), "16");
    assert_eq!(compile_and_run(r#"<?php echo bin2hex(sha1("abc",true));"#), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(compile_and_run(r#"<?php echo strlen(sha1("abc",true));"#), "20");
}
```

- [ ] **Step 2: Run to confirm failure.** `cargo test --test codegen_tests md5_sha1 2>&1 | tail -20`. The `$binary` assertions must FAIL today (md5/sha1 ignore the flag). The plain ones may pass.

- [ ] **Step 3: Reroute `__rt_md5` / `__rt_sha1`.**

Rewrite `emit_md5` (`src/codegen/runtime/strings/md5.rs`) and `emit_sha1` (`sha1.rs`) for both arches so each: places its fixed algorithm name in the algo regs (emit a private `.asciz "md5"` / `.asciz "sha1"` constant via `data`/a label, load its address+len), takes the input string in the data regs, takes the binary flag in the agreed register (ARM64 `x5`/x86_64 `r10`), and reuses the shared slot-call + `__rt_digest_to_string` path introduced in Task 1 (call into a shared core or replicate the marshalling and jump to the shared formatter). Remove the `CC_MD5`/`CC_SHA1` calls from these files.

- [ ] **Step 4: Update the `md5()` / `sha1()` builtin emitters.**

In `src/codegen/builtins/strings/md5.rs` and `sha1.rs`: keep `emit_string_arg(&args[0],…)` for the data string; additionally evaluate the optional `$binary` arg (default false) into the flag register (ARM64 `x5`/x86_64 `r10`); call `super::hash_crypto::publish_elephc_crypto_function_pointers(emitter)` before the `__rt_md5`/`__rt_sha1` call. Keep `Some(PhpType::Str)`.

- [ ] **Step 5: Swap the conditional link.**

In `src/types/checker/builtins/strings.rs`, the `"md5" | "sha1"` arm (≈line 328): replace `require_linux_builtin_library("crypto")` with `require_builtin_library("elephc_crypto")`. The arity already allows the optional `$binary` (signature is `optional(["string","binary"],1,…)`); confirm the checker arm doesn't hard-reject 2 args (if it does `args.len() != 1`, relax to `1..=2`).

- [ ] **Step 6: Build clean + run tests.** `cargo build 2>&1 | tail -5` (zero warnings); `cargo test --test codegen_tests md5_sha1 2>&1 | tail -20` → PASS. Also re-run Task 1's `hash_` tests to confirm no regression.

- [ ] **Step 7: Assembly-comment alignment** for `runtime/strings/md5.rs` and `sha1.rs` (column-81 checker). Fix any drift.

- [ ] **Step 8: Commit.**

```bash
git add src/codegen/runtime/strings/md5.rs src/codegen/runtime/strings/sha1.rs src/codegen/builtins/strings/md5.rs src/codegen/builtins/strings/sha1.rs src/types/checker/builtins/strings.rs tests/
git commit -m "feat(crypto): route md5()/sha1() through elephc-crypto and honor \$binary"
```

---

### Task 3: Phase-2 gate (regression, parity, multi-target)

- [ ] **Step 1: crc32 + phar regression (must be untouched).**

Run: `cargo test --test codegen_tests crc32 2>&1 | tail -10` and any phar tests `cargo test phar 2>&1 | tail -20`. Expected: PASS — crc32 is unchanged, phar still signs via CC_SHA1. Confirm `rg -n "CC_SHA1" src/codegen/runtime/io/phar_write.rs` still shows the 2 phar call sites (we did NOT remove them) and `rg -n "CC_MD5|CC_SHA1|CC_SHA256" src/codegen/runtime/strings/` shows NONE (hash/md5/sha1 fully migrated).

- [ ] **Step 2: Full local suite (macOS arm64).**

Run: `cargo test 2>&1 | tail -25` then `cargo test -- --include-ignored 2>&1 | tail -15`. Expected: all pass. This catches any regression in the always-emitted runtime.

- [ ] **Step 3: Linux multi-target (target-sensitive change).**

Run the Docker scripts (the change alters runtime asm, link libs, and removes CC_* from the string hashes on both arches):
```
./scripts/test-linux-x86_64.sh hash
./scripts/test-linux-x86_64.sh md5
./scripts/test-linux-arm64.sh hash
./scripts/test-linux-arm64.sh md5
```
Expected: the hashing tests pass on both Linux targets, confirming `elephc_crypto` links (no `-lcrypto` needed for hash/md5/sha1) and the runtime asm is correct per-arch. If a Docker image is missing, run with `--rebuild` once.

- [ ] **Step 4: Whitespace + warnings.**

Run: `git diff --check` (clean) and confirm `cargo build` and `cargo build --release` are warning-free.

- [ ] **Step 5: Commit any gate fixes** (if Steps 1-4 surfaced issues, fix and commit with a `fix(crypto):` message; otherwise nothing to commit).

---

## Self-Review

**Spec coverage (Phase 2 = spec Components 2+3 for the one-shot builtins):**
- Route hash/md5/sha1 through the crate via slot + publish → Tasks 1, 2. ✓
- Full algorithm set via hash() → Task 1 tests (sha512/sha3/ripemd/crc32b). ✓
- `$binary` honored (closes the latent gap) → Tasks 1, 2. ✓
- `ValueError` on unknown algorithm (catchable, PHP-exact message) → Task 1. ✓
- Conditional-link swap `crypto`→`elephc_crypto` (all targets) → Tasks 1, 2. ✓
- Parity regression for md5/sha1/sha256 → Tasks 1, 2 tests. ✓
- case-insensitive/namespaced call → Task 1 test. ✓
- crc32 untouched; phar CC_SHA1 + weak symbols + platform transform left for Phase 5 → Task 3 verification. ✓
- Multi-target (macOS arm64 local + Linux x86_64/arm64 Docker) → Task 3. ✓

**Deferred to later phases (not in this plan):** hash_hmac/hash_file/hash_equals/hash_algos (Phase 3), incremental HashContext (Phase 4), phar migration + full CommonCrypto/libcrypto removal + docs/examples/roadmap (Phase 5).

**Placeholder scan:** no TBDs. The assembly emitters are specified by contract + the exact patterns to mirror (clamp.rs for ValueError, https.rs for the slot, the existing inline hex loop) + the C-ABI marshalling registers; they are developed TDD against the concrete PHP-golden codegen tests. This is the appropriate altitude for target-aware assembly (full instruction listings would need iteration against `as`+`ld`).

**Type/name consistency:** slot `_elephc_crypto_hash_fn`; publisher `publish_elephc_crypto_function_pointers`; C symbol `elephc_crypto_hash`; binary-flag register ARM64 `x5` / x86_64 `r10` (consistent across the emitter that sets it and the runtime that reads it — both tasks must agree); link string `"elephc_crypto"`; shared formatter `__rt_digest_to_string`. All consistent across Tasks 1-2.

**Risk note for the implementer:** the binary-flag pass register is ARM64 `x5` / x86_64 `r10` — both chosen because they are NOT among the C-ABI argument registers used to call `elephc_crypto_hash` (ARM64 x0-x4; x86_64 rdi/rsi/rdx/rcx/r8). Do NOT use x86_64 `r10` for the flag — `r8` is the 5th C-ABI arg (`out_buf`). Both `x5` and `r10` are caller-saved (clobbered by the `call`), so the runtime MUST save the flag to a stack slot before invoking the slot and reload it before formatting. Task 1 Step 5.2 ("preserve the binary flag across the call") covers this — implement it on both arches.
