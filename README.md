# tokio-io-utility

[![Rust](https://github.com/NobodyXu/tokio-io-utility/actions/workflows/rust.yml/badge.svg)](https://github.com/NobodyXu/tokio-io-utility/actions/workflows/rust.yml)

[![crate.io downloads](https://img.shields.io/crates/d/tokio-io-utility)](https://crates.io/crates/tokio-io-utility)

[![crate.io version](https://img.shields.io/crates/v/tokio-io-utility)](https://crates.io/crates/tokio-io-utility)

[![docs](https://docs.rs/tokio-io-utility/badge.svg)](https://docs.rs/tokio-io-utility)

Provide some helper functions for reading into `Vec`/`Bytes`, writing all `IoSlice`s
and provides a `MpScBytesQueue` for efficient buffering and flushing using
`write_vectored`.

## How to run tests

```
./run_test.sh
```
