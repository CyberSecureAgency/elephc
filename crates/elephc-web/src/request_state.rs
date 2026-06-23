//! Purpose:
//! Owns the per-worker request/response state the `--web` bridge shares with the
//! compiled PHP runtime: the output-capture flag the runtime reads, the
//! response-body buffer the runtime appends to, and the parsed incoming request
//! statics (method, URI, path, query string, headers, body) the web prelude
//! reads via C-ABI getters. Provides `elephc_web_write`, `set_request`, all
//! request getters, and buffer lifecycle helpers.
//!
//! Called from:
//! - The compiled `--web` runtime's `__rt_stdout_write` capture branch, which
//!   calls `elephc_web_write(ptr, len)` when `elephc_web_capture` is non-zero.
//! - `crate::server::elephc_web_run`, which sets capture, clears the buffer,
//!   runs the handler, and flushes the captured body.
//! - `crate::worker`, which calls `set_request` after parsing the HTTP request
//!   and before invoking the PHP handler.
//!
//! Key details:
//! - One process per prefork worker, single-threaded: each request runs to
//!   completion on the worker's one thread, so all process-statics are race-free.
//! - All access to `static mut` items goes through raw pointers
//!   (`core::ptr::addr_of_mut!` / `core::ptr::addr_of!`), never `&mut`/`&`
//!   references, to stay clear of the `static_mut_refs` lint (a hard error under
//!   the workspace's zero-warnings gate).

use std::ffi::{c_char, CString};

extern "C" {
    /// Per-request output-capture flag defined in the compiled program's runtime
    /// `.comm` storage (`elephc_web_capture`). Non-zero routes the runtime's
    /// `__rt_stdout_write` through `elephc_web_write` instead of the plain
    /// `write(1, …)` syscall. The compiler mangles this name per target, so the
    /// clean C name here resolves to `_elephc_web_capture` on macOS and
    /// `elephc_web_capture` on Linux — matching the runtime's `.comm` and load.
    static mut elephc_web_capture: u8;
}

/// Process-static per-worker response body. Bytes echoed by the PHP handler land
/// here while capture is enabled; the server scaffold flushes it to the client
/// (currently stdout) once the handler returns.
static mut RESPONSE_BODY: Vec<u8> = Vec::new();

/// Enables or disables per-request output capture by writing the runtime's
/// extern capture flag. When `on` is true, `__rt_stdout_write` routes echo
/// output to `elephc_web_write` (the buffer below) instead of stdout.
///
/// # Safety
/// Single-threaded per worker (see module docs): the extern flag is reached only
/// through a raw pointer, never a reference to the `static mut`.
pub fn set_capture(on: bool) {
    unsafe {
        core::ptr::write(core::ptr::addr_of_mut!(elephc_web_capture), u8::from(on));
    }
}

/// Clears the response-body buffer before a request begins, so each request
/// starts with an empty body regardless of the previous request's output.
pub fn clear_body() {
    // SAFETY: single-threaded per worker; the buffer is mutated through a raw
    // pointer to avoid forming a reference to the `static mut`.
    unsafe {
        (*core::ptr::addr_of_mut!(RESPONSE_BODY)).clear();
    }
}

/// Appends `len` bytes starting at `ptr` to the per-worker response body. This
/// is the real destination for captured PHP output: the compiled runtime's
/// `__rt_stdout_write` capture branch calls this with the same C ABI as the
/// Phase-1 stub (byte pointer + length, no return value).
///
/// # Safety
/// `ptr` must point to `len` valid bytes for the duration of the call. Single-
/// threaded per worker (see module docs), so the buffer append cannot race.
#[no_mangle]
pub unsafe extern "C" fn elephc_web_write(ptr: *const u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    let bytes = core::slice::from_raw_parts(ptr, len);
    (*core::ptr::addr_of_mut!(RESPONSE_BODY)).extend_from_slice(bytes);
}

/// Takes ownership of the accumulated response body, leaving the buffer empty for
/// the next request. The server scaffold writes the returned bytes to the client.
pub fn take_body() -> Vec<u8> {
    // SAFETY: single-threaded per worker; the buffer is replaced through a raw
    // pointer to avoid forming a reference to the `static mut`.
    unsafe { core::mem::take(&mut *core::ptr::addr_of_mut!(RESPONSE_BODY)) }
}

// Per-worker current-request state. One request runs to completion on the
// worker's single thread before the next begins, so plain process statics are
// race-free (same invariant as RESPONSE_BODY).
static mut REQ_METHOD: Option<CString> = None;
static mut REQ_URI: Option<CString> = None;
static mut REQ_PATH: Option<CString> = None;
static mut REQ_QUERY: Option<CString> = None;
static mut REQ_HEADERS: Vec<(CString, CString)> = Vec::new();
static mut REQ_BODY: Vec<u8> = Vec::new();

/// Stores the parsed request for the current worker thread. Called by the
/// worker before invoking the PHP handler. Non-UTF8 / interior-NUL bytes in
/// header values are replaced (CString cannot hold a NUL), which is acceptable
/// for HTTP tokens; the raw body keeps every byte (it is exposed binary-safe).
pub(crate) fn set_request(
    method: String,
    uri: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) {
    fn cstr(s: &str) -> CString {
        CString::new(s.replace('\0', "")).unwrap_or_default()
    }
    unsafe {
        core::ptr::write(core::ptr::addr_of_mut!(REQ_METHOD), Some(cstr(&method)));
        core::ptr::write(core::ptr::addr_of_mut!(REQ_URI), Some(cstr(&uri)));
        core::ptr::write(core::ptr::addr_of_mut!(REQ_PATH), Some(cstr(&path)));
        core::ptr::write(core::ptr::addr_of_mut!(REQ_QUERY), Some(cstr(&query)));
        let hs: Vec<(CString, CString)> =
            headers.iter().map(|(n, v)| (cstr(n), cstr(v))).collect();
        core::ptr::write(core::ptr::addr_of_mut!(REQ_HEADERS), hs);
        core::ptr::write(core::ptr::addr_of_mut!(REQ_BODY), body);
    }
}

/// Returns the C-string pointer held in an Option<CString> static, or an empty
/// string pointer when unset. The pointer is valid until the static is next
/// written (i.e. until the next request) — the compiler copies it immediately.
unsafe fn opt_ptr(slot: *const Option<CString>) -> *const c_char {
    static EMPTY: [c_char; 1] = [0];
    match &*slot {
        Some(s) => s.as_ptr(),
        None => EMPTY.as_ptr(),
    }
}

/// Returns the HTTP request method (e.g. "GET"); empty string before the first request.
#[no_mangle]
pub unsafe extern "C" fn elephc_web_method() -> *const c_char {
    opt_ptr(core::ptr::addr_of!(REQ_METHOD))
}

/// Returns the raw request URI target (e.g. "/a?b=1"); empty string before the first request.
#[no_mangle]
pub unsafe extern "C" fn elephc_web_uri() -> *const c_char {
    opt_ptr(core::ptr::addr_of!(REQ_URI))
}

/// Returns the path component of the URI (e.g. "/a"); empty string before the first request.
#[no_mangle]
pub unsafe extern "C" fn elephc_web_path() -> *const c_char {
    opt_ptr(core::ptr::addr_of!(REQ_PATH))
}

/// Returns the query string component of the URI (e.g. "b=1"), without the leading "?";
/// empty string when there is no query string or before the first request.
#[no_mangle]
pub unsafe extern "C" fn elephc_web_query_string() -> *const c_char {
    opt_ptr(core::ptr::addr_of!(REQ_QUERY))
}

/// Returns the number of request headers received with the current request.
#[no_mangle]
pub unsafe extern "C" fn elephc_web_header_count() -> i64 {
    (*core::ptr::addr_of!(REQ_HEADERS)).len() as i64
}

/// Returns the name of header at index `i` (zero-based), or an empty string when out of range.
/// The pointer is valid until the next request.
#[no_mangle]
pub unsafe extern "C" fn elephc_web_header_name(i: i64) -> *const c_char {
    static EMPTY: [c_char; 1] = [0];
    let hs = &*core::ptr::addr_of!(REQ_HEADERS);
    match usize::try_from(i).ok().and_then(|i| hs.get(i)) {
        Some((n, _)) => n.as_ptr(),
        None => EMPTY.as_ptr(),
    }
}

/// Returns the value of header at index `i` (zero-based), or an empty string when out of range.
/// The pointer is valid until the next request.
#[no_mangle]
pub unsafe extern "C" fn elephc_web_header_value(i: i64) -> *const c_char {
    static EMPTY: [c_char; 1] = [0];
    let hs = &*core::ptr::addr_of!(REQ_HEADERS);
    match usize::try_from(i).ok().and_then(|i| hs.get(i)) {
        Some((_, v)) => v.as_ptr(),
        None => EMPTY.as_ptr(),
    }
}

/// Returns a pointer to the raw request body bytes (binary-safe; not NUL-terminated).
/// The pointer is valid until the next request.
#[no_mangle]
pub unsafe extern "C" fn elephc_web_body_ptr() -> *const u8 {
    (*core::ptr::addr_of!(REQ_BODY)).as_ptr()
}

/// Returns the raw request body length in bytes.
#[no_mangle]
pub unsafe extern "C" fn elephc_web_body_len() -> i64 {
    (*core::ptr::addr_of!(REQ_BODY)).len() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies set_request round-trips through the C-ABI getters.
    #[test]
    fn request_getters_round_trip() {
        use std::ffi::CStr;
        set_request(
            "POST".into(),
            "/p?x=1".into(),
            "/p".into(),
            "x=1".into(),
            vec![("Content-Type".into(), "text/plain".into())],
            b"hello".to_vec(),
        );
        unsafe {
            assert_eq!(CStr::from_ptr(elephc_web_method()).to_str().unwrap(), "POST");
            assert_eq!(CStr::from_ptr(elephc_web_uri()).to_str().unwrap(), "/p?x=1");
            assert_eq!(CStr::from_ptr(elephc_web_path()).to_str().unwrap(), "/p");
            assert_eq!(CStr::from_ptr(elephc_web_query_string()).to_str().unwrap(), "x=1");
            assert_eq!(elephc_web_header_count(), 1);
            assert_eq!(CStr::from_ptr(elephc_web_header_name(0)).to_str().unwrap(), "Content-Type");
            assert_eq!(CStr::from_ptr(elephc_web_header_value(0)).to_str().unwrap(), "text/plain");
            assert_eq!(elephc_web_body_len(), 5);
            let body = std::slice::from_raw_parts(elephc_web_body_ptr(), 5);
            assert_eq!(body, b"hello");
        }
    }
}
