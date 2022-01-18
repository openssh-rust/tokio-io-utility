#!/bin/bash -ex

cargo test --all-features

export RUSTFLAGS='-Zsanitizer=address'
exec cargo +nightly test --all-features async_read_utility::tests::test
