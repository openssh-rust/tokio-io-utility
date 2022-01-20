#!/bin/bash -ex

cargo test --all-features -- --nocapture

export RUSTFLAGS='-Zsanitizer=address'
cargo +nightly test --all-features async_read_utility::tests::test -- --nocapture
cargo +nightly test --all-features queue -- --nocapture

export RUSTFLAGS='-Zsanitizer=thread' 

for _ in $(seq 1 10); do
    cargo +nightly test \
        -Z build-std \
        --target --target $(uname -m)-unknown-linux-gnu \
        --all-features queue::tests::test_par -- --nocapture
done
