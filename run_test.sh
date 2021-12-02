#!/bin/bash -ex

cargo test

export RUSTFLAGS='-Zsanitizer=address'
exec cargo +nightly test async_read_utility::tests::test
