#!/bin/bash -ex

cargo test --all-features -- --nocapture

export RUSTFLAGS='-Zsanitizer=address'
cargo +nightly test --all-features async_read_utility::tests::test -- --nocapture
exec cargo +nightly test --all-features queue -- --nocapture
