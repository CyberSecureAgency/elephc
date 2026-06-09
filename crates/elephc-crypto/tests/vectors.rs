//! Purpose:
//! Integration tests validating elephc-crypto digests against published test
//! vectors and PHP golden values.
//!
//! Called from:
//! - `cargo test -p elephc-crypto` through Rust's test harness.
//!
//! Key details:
//! - Calls the C ABI functions directly (the crate links as an rlib in tests).
//! - Vectors are NIST/RFC published values; non-crypto checksums are cross-checked
//!   against `php -r 'echo hash(...);'`.

use elephc_crypto::*;

/// Convenience: run the one-shot C ABI and return the lowercase hex digest, or
/// `None` when the algorithm is unknown (ABI returns -1).
fn hash_hex(algo: &str, data: &[u8]) -> Option<String> {
    let mut out = [0u8; 64];
    let n = unsafe {
        elephc_crypto_hash(
            algo.as_ptr(),
            algo.len(),
            data.as_ptr(),
            data.len(),
            out.as_mut_ptr(),
        )
    };
    if n < 0 {
        return None;
    }
    Some(out[..n as usize].iter().map(|b| format!("{:02x}", b)).collect())
}

#[test]
fn crypto_one_shot_known_vectors() {
    assert_eq!(hash_hex("md5", b"").unwrap(), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(hash_hex("md5", b"abc").unwrap(), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(hash_hex("sha1", b"abc").unwrap(), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(
        hash_hex("sha256", b"abc").unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hash_hex("sha512", b"abc").unwrap(),
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
    );
    assert_eq!(
        hash_hex("sha3-256", b"abc").unwrap(),
        "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"
    );
    assert_eq!(hash_hex("ripemd160", b"abc").unwrap(), "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc");
}

#[test]
fn unknown_algorithm_returns_negative() {
    assert!(hash_hex("tiger", b"abc").is_none());
    assert!(hash_hex("not-a-hash", b"abc").is_none());
}

#[test]
fn non_crypto_checksum_vectors_match_php() {
    // Golden values produced by `php -r 'echo hash($algo, "abc");'` (PHP 8.4).
    assert_eq!(hash_hex("crc32", b"abc").unwrap(), "73bb8c64");
    assert_eq!(hash_hex("crc32b", b"abc").unwrap(), "352441c2");
    assert_eq!(hash_hex("adler32", b"abc").unwrap(), "024d0127");
    assert_eq!(hash_hex("fnv132", b"abc").unwrap(), "439c2f4b");
    assert_eq!(hash_hex("fnv1a32", b"abc").unwrap(), "1a47e90b");
    assert_eq!(hash_hex("fnv164", b"abc").unwrap(), "d8dcca186bafadcb");
    assert_eq!(hash_hex("fnv1a64", b"abc").unwrap(), "e71fa2190541574b");
    assert_eq!(hash_hex("joaat", b"abc").unwrap(), "ed131f5b");
}

#[test]
fn non_crypto_checksum_empty_string_vectors_match_php() {
    // PHP 8.4 `hash($algo, "")` golden values.
    assert_eq!(hash_hex("crc32", b"").unwrap(), "00000000");
    assert_eq!(hash_hex("crc32b", b"").unwrap(), "00000000");
    assert_eq!(hash_hex("adler32", b"").unwrap(), "00000001");
    assert_eq!(hash_hex("fnv132", b"").unwrap(), "811c9dc5");
    assert_eq!(hash_hex("fnv1a32", b"").unwrap(), "811c9dc5");
    assert_eq!(hash_hex("fnv164", b"").unwrap(), "cbf29ce484222325");
    assert_eq!(hash_hex("fnv1a64", b"").unwrap(), "cbf29ce484222325");
    assert_eq!(hash_hex("joaat", b"").unwrap(), "00000000");
}

#[test]
fn all_algorithms_produce_correct_digest_length() {
    // (algorithm name, raw digest size in bytes)
    let cases: &[(&str, usize)] = &[
        ("md2", 16), ("md4", 16), ("md5", 16), ("sha1", 20),
        ("sha224", 28), ("sha256", 32), ("sha384", 48), ("sha512", 64),
        ("sha512/224", 28), ("sha512/256", 32),
        ("sha3-224", 28), ("sha3-256", 32), ("sha3-384", 48), ("sha3-512", 64),
        ("ripemd128", 16), ("ripemd160", 20), ("ripemd256", 32), ("ripemd320", 40),
        ("whirlpool", 64), ("blake2b512", 64), ("blake2s256", 32),
        ("crc32", 4), ("crc32b", 4), ("adler32", 4),
        ("fnv132", 4), ("fnv1a32", 4), ("fnv164", 8), ("fnv1a64", 8), ("joaat", 4),
    ];
    for (algo, len) in cases {
        let hex = hash_hex(algo, b"the quick brown fox")
            .unwrap_or_else(|| panic!("algorithm {algo} returned unknown (-1)"));
        assert_eq!(hex.len(), len * 2, "wrong digest length for {algo}");
    }
}
