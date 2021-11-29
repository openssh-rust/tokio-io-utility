# tokio-async-write-utility

[![Rust](https://github.com/NobodyXu/tokio-async-write-utility/actions/workflows/rust.yml/badge.svg)](https://github.com/NobodyXu/tokio-async-write-utility/actions/workflows/rust.yml)

[![crate.io downloads](https://img.shields.io/crates/d/tokio-async-write-utility)](https://crates.io/crates/tokio-async-write-utility)

[![crate.io version](https://img.shields.io/crates/v/tokio-async-write-utility)](https://crates.io/crates/tokio-async-write-utility)

[![docs](https://docs.rs/tokio-async-write-utility/badge.svg)](https://docs.rs/tokio-async-write-utility)

Some helper functions for types impl `AsyncWrite`.

It currently provides the following functions through `trait AsyncWriteUtility`:
 - `fn poll_write_vectored_all(Pin<&mut Self>, &mut Context<'_>, &mut [IoSlice<'_>]) -> io::Result<()>`
 - `fn write_vectored_all(&mut self, &mut [IoSlice<'_>]) -> WriteVectorizedAll`
   
   which is equivalent to:

   ```
   async fn write_vectored_all(&mut self, &mut [IoSlice<'_>],) -> io::Result<()>
   ```
