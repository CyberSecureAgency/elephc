//! Purpose:
//! Regression tests for transient `_concat_buf` string results passed as call arguments.
//!
//! Called from:
//! - `cargo test` through Rust's test harness.
//!
//! Key details:
//! - A string builtin (`strtoupper`/`md5`/…) or a runtime concatenation materializes its
//!   result into the shared `_concat_buf` scratch and returns a borrowed slice. Before the
//!   fix, passing such a slice as a function/method/closure argument produced garbage: the
//!   callee's first statement reset `_concat_off` to 0 and the callee's own concats
//!   overwrote the caller's slice bytes. The fix has each frame reset `_concat_off` to the
//!   base it inherited from the caller (the high-water mark below which the caller's slice
//!   lives), so the callee appends above the caller's slice instead of clobbering it.
//!   These assertions fail on the pre-fix codegen (garbage output) and pass after it.

use super::*;

/// A `_concat_buf`-slice builtin result (`strtoupper`) passed to a user function must
/// survive the callee's per-statement concat reset and read back as the correct value.
#[test]
fn test_transient_builtin_string_arg_to_function() {
    let out = compile_and_run(
        r#"<?php function t($v){ return "[".$v."]"; } echo t(strtoupper("xy"));"#,
    );
    assert_eq!(out, "[XY]");
}

/// A runtime-concatenation result (`"ab".$d`) passed as an argument must survive the
/// callee's concat reset; this is the canonical minimal repro of the original bug.
#[test]
fn test_runtime_concat_string_arg_to_function() {
    let out = compile_and_run(
        r#"<?php function t($v){ return "[".$v."]"; } $d = "cd"; echo t("ab".$d);"#,
    );
    assert_eq!(out, "[abcd]");
}

/// The fix must cover method calls (vtable dispatch), not just plain functions: a
/// transient string argument to a method must survive the method body's concat reset.
#[test]
fn test_transient_string_arg_to_method() {
    let out = compile_and_run(
        r#"<?php class R { public function h(string $s): string { return "<".$s.">"; } } $r = new R(); echo $r->h(strtoupper("hi"));"#,
    );
    assert_eq!(out, "<HI>");
}

/// The fix must cover closures: a transient string argument to a closure must survive the
/// closure body's concat reset.
#[test]
fn test_transient_string_arg_to_closure() {
    let out = compile_and_run(
        r#"<?php $f = function($x){ return "{".$x."}"; }; echo $f(strtolower("ZZ"));"#,
    );
    assert_eq!(out, "{zz}");
}

/// Chained pass-through: `outer` preserves its inherited slice, then passes it to `inner`,
/// which preserves it again above `outer`'s region. Verifies the base is per-frame.
#[test]
fn test_transient_string_arg_chained_passthrough() {
    let out = compile_and_run(
        r#"<?php function inner($a){ return "(".$a.")"; } function outer($b){ return inner($b); } echo outer(strtoupper("qq"));"#,
    );
    assert_eq!(out, "(QQ)");
}

/// A longer `_concat_buf` builtin result (`md5`, 32 bytes) passed as an argument must be
/// read back intact, guarding against partial-overwrite variants of the bug.
#[test]
fn test_transient_md5_string_arg_to_function() {
    let out = compile_and_run(
        r#"<?php function t($v){ return "[".$v."]"; } echo t(md5("x"));"#,
    );
    assert_eq!(out, "[9dd4e461268c8034f5c8564e155c67a6]");
}

/// Two transient string arguments in one call must both survive: the callee's base sits
/// above both caller slices, so neither is clobbered.
#[test]
fn test_two_transient_string_args() {
    let out = compile_and_run(
        r#"<?php function j($a, $b){ return $a."-".$b; } echo j(strtoupper("ab"), strtolower("CD"));"#,
    );
    assert_eq!(out, "AB-cd");
}
