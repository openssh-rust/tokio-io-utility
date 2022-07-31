use std::future::Future;
use std::io::Result;
use std::marker::Unpin;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::AsyncRead;

mod inner;
pub use inner::*;

/// Returned future of [`read_to_vec`].
#[derive(Debug)]
pub struct ReadToVecFuture<'a, T: ?Sized>(ReadExactToVecFuture<'a, T>);
impl<T: AsyncRead + ?Sized + Unpin> Future for ReadToVecFuture<'_, T> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

/// Try to fill data from `reader` into the spare capacity of `vec`.
///
/// It can be used to implement buffering.
///
/// Return [`ErrorKind::UnexpectedEof`] on Eof.
///
/// NOTE that this function does not modify any existing data.
///
/// # Cancel safety
///
/// It is cancel safe and dropping the returned future will not stop the
/// wakeup from happening.
pub fn read_to_vec<'a, T: AsyncRead + ?Sized + Unpin>(
    reader: &'a mut T,
    vec: &'a mut Vec<u8>,
) -> ReadToVecFuture<'a, T> {
    let spare = vec.capacity() - vec.len();

    ReadToVecFuture(read_exact_to_vec(reader, vec, spare))
}

/// Returned future of [`read_exact_to_vec`].
#[derive(Debug)]
pub struct ReadExactToVecFuture<'a, T: ?Sized>(ReadToVecRngFuture<'a, T>);
impl<T: AsyncRead + ?Sized + Unpin> Future for ReadExactToVecFuture<'_, T> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

/// * `nread` - bytes to read in
///
/// Return [`ErrorKind::UnexpectedEof`] on Eof.
///
/// NOTE that this function does not modify any existing data.
///
/// # Cancel safety
///
/// It is cancel safe and dropping the returned future will not stop the
/// wakeup from happening.
pub fn read_exact_to_vec<'a, T: AsyncRead + ?Sized + Unpin>(
    reader: &'a mut T,
    vec: &'a mut Vec<u8>,
    nread: usize,
) -> ReadExactToVecFuture<'a, T> {
    ReadExactToVecFuture(read_to_vec_rng(reader, vec, nread..=nread))
}

#[derive(Debug)]
pub struct ReadToVecRngFuture<'a, Reader: ?Sized>(ReadToContainerRngFuture<'a, Vec<u8>, Reader>);

impl<Reader: AsyncRead + ?Sized + Unpin> Future for ReadToVecRngFuture<'_, Reader> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

/// * `rng` - The start of the range specify the minimum of bytes to read in,
///           while the end of the range specify the maximum of bytes that
///           can be read in.
///           If the lower bound is not specified, it is default to 0.
///           If the upper bound is not specified, it is default to the
///           capacity of `bytes`.
///
/// Return [`ErrorKind::UnexpectedEof`] on Eof.
///
/// NOTE that this function does not modify any existing data.
///
/// # Cancel safety
///
/// It is cancel safe and dropping the returned future will not stop the
/// wakeup from happening.
pub fn read_to_vec_rng<'a, T: AsyncRead + ?Sized + Unpin>(
    reader: &'a mut T,
    vec: &'a mut Vec<u8>,
    rng: impl std::ops::RangeBounds<usize>,
) -> ReadToVecRngFuture<'a, T> {
    ReadToVecRngFuture(read_to_container_rng(reader, vec, rng))
}

/// Returned future of [`read_exact_to_vec`].
#[derive(Debug)]
#[cfg(feature = "read-exact-to-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
pub struct ReadExactToBytesFuture<'a, T: ?Sized>(ReadToBytesRngFuture<'a, T>);

#[cfg(feature = "read-exact-to-bytes")]
impl<T: AsyncRead + ?Sized + Unpin> Future for ReadExactToBytesFuture<'_, T> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

#[cfg(feature = "read-exact-to-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
/// * `nread` - bytes to read in
///
/// Return [`ErrorKind::UnexpectedEof`] on Eof.
///
/// NOTE that this function does not modify any existing data.
///
/// # Cancel safety
///
/// It is cancel safe and dropping the returned future will not stop the
/// wakeup from happening.
pub fn read_exact_to_bytes<'a, T: AsyncRead + ?Sized + Unpin>(
    reader: &'a mut T,
    bytes: &'a mut bytes::BytesMut,
    nread: usize,
) -> ReadExactToBytesFuture<'a, T> {
    bytes.reserve(nread);

    ReadExactToBytesFuture(read_to_bytes_rng(reader, bytes, nread..=nread))
}

/// Returned future of [`read_exact_to_vec`].
#[derive(Debug)]
#[cfg(feature = "read-exact-to-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
pub struct ReadToBytesRngFuture<'a, Reader: ?Sized>(
    ReadToContainerRngFuture<'a, bytes::BytesMut, Reader>,
);

#[cfg(feature = "read-exact-to-bytes")]
impl<Reader: AsyncRead + ?Sized + Unpin> Future for ReadToBytesRngFuture<'_, Reader> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

#[cfg(feature = "read-exact-to-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
/// * `rng` - The start of the range specify the minimum of bytes to read in,
///           while the end of the range specify the maximum of bytes that
///           can be read in.
///           If the lower bound is not specified, it is default to 0.
///           If the upper bound is not specified, it is default to the
///           capacity of `bytes`.
///
/// Return [`ErrorKind::UnexpectedEof`] on Eof.
///
/// NOTE that this function does not modify any existing data.
///
/// # Cancel safety
///
/// It is cancel safe and dropping the returned future will not stop the
/// wakeup from happening.
pub fn read_to_bytes_rng<'a, T: AsyncRead + ?Sized + Unpin>(
    reader: &'a mut T,
    bytes: &'a mut bytes::BytesMut,
    rng: impl std::ops::RangeBounds<usize>,
) -> ReadToBytesRngFuture<'a, T> {
    ReadToBytesRngFuture(read_to_container_rng(reader, bytes, rng))
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::AsyncWriteExt;

    #[test]
    fn test_read_to_vec() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (mut r, mut w) = tokio_pipe::pipe().unwrap();

                for n in 1..=255 {
                    w.write_u8(n).await.unwrap();
                }

                let mut buffer = Vec::with_capacity(255);

                read_to_vec(&mut r, &mut buffer).await.unwrap();

                assert_eq!(buffer.len(), 255);
                for (i, each) in buffer.iter().enumerate() {
                    assert_eq!(*each as usize, i + 1);
                }
            });
    }

    #[test]
    fn test_read_exact_to_vec() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (mut r, mut w) = tokio_pipe::pipe().unwrap();

                let w_task = tokio::spawn(async move {
                    for n in 1..=255 {
                        w.write_u8(n).await.unwrap();
                    }
                });

                let r_task = tokio::spawn(async move {
                    let mut buffer = vec![0];

                    read_exact_to_vec(&mut r, &mut buffer, 255).await.unwrap();

                    for (i, each) in buffer.iter().enumerate() {
                        assert_eq!(*each as usize, i);
                    }
                });
                r_task.await.unwrap();
                w_task.await.unwrap();
            });
    }

    #[cfg(feature = "read-exact-to-bytes")]
    #[test]
    fn test_read_exact_to_bytes() {
        use bytes::BufMut;

        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (mut r, mut w) = tokio_pipe::pipe().unwrap();

                let w_task = tokio::spawn(async move {
                    for n in 1..=255 {
                        w.write_u8(n).await.unwrap();
                    }
                });

                let r_task = tokio::spawn(async move {
                    let mut buffer = bytes::BytesMut::new();
                    buffer.put_u8(0);

                    read_exact_to_bytes(&mut r, &mut buffer, 255).await.unwrap();

                    for (i, each) in buffer.iter().enumerate() {
                        assert_eq!(*each as usize, i);
                    }
                });
                r_task.await.unwrap();
                w_task.await.unwrap();
            });
    }
}
