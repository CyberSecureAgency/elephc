//! Purpose:
//! Pure-Rust hashing/HMAC bridge staticlib for elephc's PHP hash() family.
//! Exposes a C ABI of raw-digest functions keyed by algorithm name, consumed by
//! compiled PHP binaries via function-pointer slots (see src/codegen runtime).
//!
//! Called from:
//! - Compiled PHP program assembly through the `_elephc_crypto_*_fn` slots.
//! - `cargo test -p elephc-crypto` (the rlib) for in-isolation validation.
//!
//! Key details:
//! - All ABI functions are `#[no_mangle] pub extern "C"`; raw digests are written
//!   into a caller-provided 64-byte buffer (max digest size across supported algos).
//! - `ctx` handles are thin pointers to a boxed `HashCtx`; `final`/`free` own them.

mod algos;
mod hmac;

pub use algos::HashState;
use algos::make;

/// Builds a byte slice from a possibly-null/zero-length C pointer pair.
unsafe fn slice<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(ptr, len)
    }
}

/// Reads a UTF-8 algorithm name from a C pointer pair (lossy on invalid UTF-8,
/// which simply fails to match any known algorithm).
unsafe fn name_str<'a>(ptr: *const u8, len: usize) -> std::borrow::Cow<'a, str> {
    String::from_utf8_lossy(slice(ptr, len))
}

/// Computes a one-shot raw HMAC of `data` keyed by `key` under `name`, writing
/// the digest to `out` (64-byte buffer). Returns the digest length, or -1 for an
/// unknown algorithm or a non-crypto checksum (which PHP rejects for HMAC).
///
/// # Safety
/// All pointers must be valid for their stated lengths; `out` must hold 64 bytes.
#[no_mangle]
pub unsafe extern "C" fn elephc_crypto_hmac(
    name_ptr: *const u8,
    name_len: usize,
    key_ptr: *const u8,
    key_len: usize,
    data_ptr: *const u8,
    data_len: usize,
    out_ptr: *mut u8,
) -> isize {
    let name = name_str(name_ptr, name_len);
    let digest = match hmac::hmac(&name, slice(key_ptr, key_len), slice(data_ptr, data_len)) {
        Some(d) => d,
        None => return -1,
    };
    std::ptr::copy_nonoverlapping(digest.as_ptr(), out_ptr, digest.len());
    digest.len() as isize
}

/// Computes a one-shot raw digest of `data` under `name`, writing the bytes to
/// `out` (caller guarantees a 64-byte buffer). Returns the digest length, or -1
/// for an unknown algorithm.
///
/// # Safety
/// All pointers must be valid for their stated lengths; `out` must hold 64 bytes.
#[no_mangle]
pub unsafe extern "C" fn elephc_crypto_hash(
    name_ptr: *const u8,
    name_len: usize,
    data_ptr: *const u8,
    data_len: usize,
    out_ptr: *mut u8,
) -> isize {
    let name = name_str(name_ptr, name_len);
    let mut st = match make(&name) {
        Some(s) => s,
        None => return -1,
    };
    st.update(slice(data_ptr, data_len));
    let digest = st.finalize_box();
    std::ptr::copy_nonoverlapping(digest.as_ptr(), out_ptr, digest.len());
    digest.len() as isize
}
