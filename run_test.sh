#!/bin/bash -ex

rep=$(seq 1 10)

for _ in $rep; do
    cargo test --all-features -- --nocapture
done

export RUSTFLAGS='-Zsanitizer=address'
cargo +nightly test --all-features async_read_utility::tests::test -- --nocapture

for _ in $rep; do
    cargo +nightly test --all-features queue -- --nocapture
done

export RUSTFLAGS='-Zsanitizer=thread' 

for _ in $rep; do
    cargo +nightly test \
        -Z build-std \
        --target $(uname -m)-unknown-linux-gnu \
        --all-features queue::tests::test_par -- --nocapture
done

#unset RUSTFLAGS
#export MIRIFLAGS="-Zmiri-disable-isolation"
#exec cargo +nightly miri test \
#    -Z build-std \
#    --target $(uname -m)-unknown-linux-gnu \
#    --all-features queue -- --nocapture
