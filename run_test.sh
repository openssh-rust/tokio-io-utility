#!/bin/bash -ex

arch=$(uname -m)
if [ "$arch" = "arm64" ]; then
    arch=aarch64
fi

os=unknown-linux-gnu
if uname -v | grep -s Darwin >/dev/null; then
    os=apple-darwin
fi

target="$arch-$os"

rep=$(seq 1 10)

for _ in $rep; do
    cargo test --all-features -- --nocapture
done

export RUSTFLAGS='-Zsanitizer=address'
cargo +nightly test --all-features async_read_utility -- --nocapture
cargo +nightly test --all-features async_write_utility -- --nocapture
cargo +nightly test --all-features io_slice_ext -- --nocapture
cargo +nightly test --all-features reusable_io_slices -- --nocapture
cargo +nightly test --all-features init_maybeuninit_io_slice -- --nocapture

for _ in $rep; do
    cargo +nightly test --all-features queue -- --nocapture
done

# thread sanitizer reports false positive in crossbeam
#export RUSTFLAGS='-Zsanitizer=thread' 
#
#for _ in $rep; do
#    cargo +nightly test \
#        -Z build-std \
#        --target "$target" \
#        --all-features queue::tests::test_par -- --nocapture
#done

unset RUSTFLAGS
export MIRIFLAGS="-Zmiri-disable-isolation"

cargo +nightly miri test \
    -Z build-std \
    --target "$target" \
    --all-features io_slice_ext -- --nocapture

cargo +nightly miri test \
    -Z build-std \
    --target "$target" \
    --all-features reusable_io_slices -- --nocapture

cargo +nightly miri test \
    -Z build-std \
    --target "$target" \
    --all-features init_maybeuninit_io_slice -- --nocapture

exec cargo +nightly miri test \
    -Z build-std \
    --target "$target" \
    --all-features queue::tests::test_seq -- --nocapture
