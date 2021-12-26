# tokio-io-utility

[![Rust](https://github.com/NobodyXu/tokio-io-utility/actions/workflows/rust.yml/badge.svg)](https://github.com/NobodyXu/tokio-io-utility/actions/workflows/rust.yml)

[![crate.io downloads](https://img.shields.io/crates/d/tokio-io-utility)](https://crates.io/crates/tokio-io-utility)

[![crate.io version](https://img.shields.io/crates/v/tokio-io-utility)](https://crates.io/crates/tokio-io-utility)

[![docs](https://docs.rs/tokio-io-utility/badge.svg)](https://docs.rs/tokio-io-utility)

Some helper functions.

`trait AsyncWriteUtility` provides for all types implement `AsyncWrite`:
 - `fn poll_write_vectored_all(Pin<&mut Self>, &mut Context<'_>, &mut [IoSlice<'_>]) -> io::Result<()>`
 - `fn write_vectored_all(&mut self, &mut [IoSlice<'_>]) -> WriteVectorizedAll`
   
   which is equivalent to:

   ```
   async fn write_vectored_all(&mut self, &mut [IoSlice<'_>],) -> io::Result<()>
   ```
`read_exact_to_vec` read in data from any type implements `AsyncRead` into `Vec<u8>`
without having to initialize the bytes first.

## How to run tests

```
./run_test.sh
```
