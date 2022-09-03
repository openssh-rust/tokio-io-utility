use super::{read_to_container_rng, Container, ReadToContainerRngFuture};

use std::{
    future::Future,
    io::Result,
    marker::Unpin,
    mem::MaybeUninit,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{BufMut, BytesMut};
use tokio::io::{AsyncRead, ReadBuf};

#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
impl Container for BytesMut {
    fn reserve(&mut self, n: usize) {
        BytesMut::reserve(self, n)
    }

    fn len(&self) -> usize {
        BytesMut::len(self)
    }

    fn capacity(&self) -> usize {
        BytesMut::capacity(self)
    }

    unsafe fn spare_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        self.chunk_mut().as_uninit_slice_mut()
    }

    unsafe fn advance(&mut self, n: usize) {
        self.advance_mut(n)
    }
}

/// Returned future of [`read_exact_to_bytes`].
#[derive(Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
pub struct ReadExactToBytesFuture<'a, R: ?Sized>(ReadToBytesRngFuture<'a, R>);

impl<R: AsyncRead + ?Sized + Unpin> Future for ReadExactToBytesFuture<'_, R> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

/// * `nread` - bytes to read in
///
/// Return [`std::io::ErrorKind::UnexpectedEof`] on Eof.
///
/// NOTE that this function does not modify any existing data.
///
/// # Cancel safety
///
/// It is cancel safe and dropping the returned future will not stop the
/// wakeup from happening.
#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
pub fn read_exact_to_bytes<'a, R: AsyncRead + ?Sized + Unpin>(
    reader: &'a mut R,
    bytes: &'a mut BytesMut,
    nread: usize,
) -> ReadExactToBytesFuture<'a, R> {
    ReadExactToBytesFuture(read_to_bytes_rng(reader, bytes, nread..=nread))
}

/// Returned future of [`read_to_bytes_rng`].
#[derive(Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
pub struct ReadToBytesRngFuture<'a, Reader: ?Sized>(ReadToContainerRngFuture<'a, BytesMut, Reader>);

impl<Reader: AsyncRead + ?Sized + Unpin> Future for ReadToBytesRngFuture<'_, Reader> {
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
///           The lower bound must not be larger than the upper bound.
///
/// Return [`std::io::ErrorKind::UnexpectedEof`] on Eof.
///
/// NOTE that this function does not modify any existing data.
///
/// # Cancel safety
///
/// It is cancel safe and dropping the returned future will not stop the
/// wakeup from happening.
#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
pub fn read_to_bytes_rng<'a, R: AsyncRead + ?Sized + Unpin>(
    reader: &'a mut R,
    bytes: &'a mut BytesMut,
    rng: impl std::ops::RangeBounds<usize>,
) -> ReadToBytesRngFuture<'a, R> {
    ReadToBytesRngFuture(read_to_container_rng(reader, bytes, rng))
}

/// Return future of [`read_to_bytes_until_end`]
///
/// Return number of bytes read in on `Poll::Ready`.
#[derive(Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
pub struct ReadToBytesUntilEndFuture<'a, Reader: ?Sized> {
    reader: &'a mut Reader,
    bytes: &'a mut BytesMut,
    cnt: usize,
}
impl<Reader: AsyncRead + ?Sized + Unpin> ReadToBytesUntilEndFuture<'_, Reader> {
    /// Read into the bytes and adjust the internal length of it.
    /// Return 0 on EOF.
    fn poll_read_to_end(&mut self, cx: &mut Context<'_>) -> Poll<Result<usize>> {
        // This uses an adaptive system to extend the vector when it fills. We want to
        // avoid paying to allocate and zero a huge chunk of memory if the reader only
        // has 4 bytes while still making large reads if the reader does have a ton
        // of data to return. Simply tacking on an extra DEFAULT_BUF_SIZE space every
        // time is 4,500 times (!) slower than this if the reader has a very small
        // amount of data to return.
        self.bytes.reserve(32);

        // Safety:
        //
        // The slice returned is not read from and only uninitialized bytes
        // would be written to it.
        let mut read_buf = ReadBuf::uninit(unsafe { self.bytes.spare_mut() });
        ready!(Pin::new(&mut *self.reader).poll_read(cx, &mut read_buf))?;

        let filled = read_buf.filled().len();

        // safety:
        //
        // `read_buf.filled().len()` return number of bytes read in.
        unsafe { self.bytes.advance(filled) };

        Poll::Ready(Ok(filled))
    }
}
impl<Reader: AsyncRead + ?Sized + Unpin> Future for ReadToBytesUntilEndFuture<'_, Reader> {
    type Output = Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = Pin::into_inner(self);

        loop {
            let n = ready!(this.poll_read_to_end(cx))?;
            if n == 0 {
                break Poll::Ready(Ok(this.cnt));
            }
            this.cnt += n;
        }
    }
}

/// NOTE that this function does not modify any existing data.
///
/// # Cancel safety
///
/// It is cancel safe and dropping the returned future will not stop the
/// wakeup from happening.
pub fn read_to_bytes_until_end<'a, R: AsyncRead + ?Sized + Unpin>(
    reader: &'a mut R,
    bytes: &'a mut BytesMut,
) -> ReadToBytesUntilEndFuture<'a, R> {
    ReadToBytesUntilEndFuture {
        reader,
        bytes,
        cnt: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[test]
    fn test_read_exact_to_bytes() {
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

    #[test]
    fn test_read_to_bytes_until_end() {
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

                    assert_eq!(
                        read_to_bytes_until_end(&mut r, &mut buffer).await.unwrap(),
                        255
                    );

                    for (i, each) in buffer.iter().enumerate() {
                        assert_eq!(*each as usize, i);
                    }
                });
                r_task.await.unwrap();
                w_task.await.unwrap();
            });
    }
}
