//! Exact-byte verification and cache capacity checks.

use griz_core::{ContentHashCache, content_hash_bytes};

#[test]
fn cache_reuses_only_identical_owned_bytes() {
    let cache = ContentHashCache::new(8);
    let mut input = b"abc".to_vec();
    assert_eq!(
        cache.hash(&input),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        cache.hash(&input),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    input[2] = b'd';
    assert_eq!(
        cache.hash(&input),
        "a52d159f262b2c6ddb724a61840befc36eb30c88877a4030b65cbe86298449c9"
    );
    assert_eq!(
        cache.hash(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(cache.cached_bytes(), Some(6));
}

#[test]
fn cache_capacity_and_oversized_inputs_follow_actual_buffers() {
    let cache = ContentHashCache::new(8);
    assert_eq!(
        cache.hash(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        cache.hash(b"def"),
        "cb8379ac2098aa165029e3938a51da0bcecfc008fd6795f401178647f96c5b34"
    );
    assert_eq!(cache.cached_bytes(), Some(6));
    assert_eq!(
        cache.hash(b"ghi"),
        "50ae61e841fac4e8f9e40baf2ad36ec868922ea48368c18f9535e47db56dd7fb"
    );
    assert_eq!(cache.cached_bytes(), Some(6));
    assert_eq!(
        cache.hash(b"larger than capacity"),
        "67d1acb9cf1906313212db7332672088fc21ec63b4eb7d8bdb9783142de29690"
    );
    assert_eq!(cache.cached_bytes(), Some(6));
}

#[test]
fn global_cache_detects_a_same_length_change_at_the_last_byte() {
    let mut input = vec![b'a'; 2 * 1024 * 1024];
    assert_eq!(
        content_hash_bytes(&input),
        "5256ec18f11624025905d057d6befb03d77b243511ac5f77ed5e0221ce6d84b5"
    );
    assert_eq!(
        content_hash_bytes(&input),
        "5256ec18f11624025905d057d6befb03d77b243511ac5f77ed5e0221ce6d84b5"
    );
    if let Some(last) = input.last_mut() {
        *last = b'b';
    }
    assert_eq!(
        content_hash_bytes(&input),
        "7b9a785ae4c9b608c95d7eb7d083d263253f81d5b38ccd6be5b284b503042b15"
    );
}

#[test]
#[ignore = "manual release-mode cache churn measurement"]
fn cache_churn_measurement() {
    use std::time::Instant;
    let cache = ContentHashCache::new(32 * 1024 * 1024);
    let mut input = vec![b'a'; 7_340_480];
    let expected = [
        "4640c6016ca43abb360b8787d183df4032a31f5238c4cdc492918377d0351dbb",
        "54c090643635cdf5198110299c2948a88022acd0ff6666ed3852ce5d2f8332a5",
        "0ceaadada8577e47295a97041206d980ca05a27bf5a5a7b85a3f80f513b54308",
        "1fbb48bbce9a6bbca4497b7cf1510c9f6f7d568ca53fe01dbedbe5865cd5bc17",
        "08facc5491043131bf64b8b08e2ae6a38ae570f577b15b129f191b2113bffe38",
    ];
    let mut samples = Vec::new();
    for _ in 0..11 {
        for (last, hash) in (0_u8..5).zip(expected) {
            input[7_340_479] = last;
            let start = Instant::now();
            let actual = cache.hash(&input);
            samples.push(start.elapsed().as_nanos());
            assert_eq!(actual, hash);
            assert!(
                cache
                    .cached_bytes()
                    .is_some_and(|bytes| bytes <= 32 * 1024 * 1024)
            );
        }
    }
    samples.sort_unstable();
    println!("profile stage=cache_churn median_ns={}", samples[27]);
}
