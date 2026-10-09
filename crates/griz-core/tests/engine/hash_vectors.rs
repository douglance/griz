//! Independently computed SHA-256 vectors across padding and binary inputs.

use griz_core::{content_hash, content_hash_bytes};

#[test]
fn sha256_matches_independent_vectors() {
    assert_eq!(
        content_hash(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        content_hash("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let binary: Vec<u8> = (0..=255).collect();
    assert_eq!(
        content_hash_bytes(&binary),
        "40aff2e9d2d8922e47afd4648e6967497158785fbd1da870e7110266bf944880"
    );
    for (length, expected) in [
        (
            1,
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb",
        ),
        (
            55,
            "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
        ),
        (
            56,
            "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a",
        ),
        (
            63,
            "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34",
        ),
        (
            64,
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb",
        ),
        (
            65,
            "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0",
        ),
        (
            1_000_000,
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
        ),
    ] {
        assert_eq!(
            content_hash_bytes(&vec![b'a'; length]),
            expected,
            "length={length}"
        );
    }
}
