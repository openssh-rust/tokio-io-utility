use std::future::Future;
use std::io::Result;
use std::marker::Unpin;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::slice::from_raw_parts_mut;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, ReadBuf};

macro_rules! ready {
    ($e:expr) => {
        match $e {
            Poll::Ready(t) => t,
            Poll::Pending => return Poll::Pending,
        }
    };
}

/// Returned future of [`read_exact_to_vec`].
#[derive(Debug)]
pub struct ReadExactToVecFuture<'a, T: ?Sized> {
    reader: &'a mut T,
    vec: &'a mut Vec<u8>,
    nread: usize,
}
impl<T: AsyncRead + ?Sized + Unpin> Future for ReadExactToVecFuture<'_, T> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        let reader = &mut *this.reader;
        let vec = &mut *this.vec;
        let nread = &mut this.nread;

        while *nread > 0 {
            let ptr = vec.as_mut_ptr() as *mut MaybeUninit<u8>;
            let len = vec.len();

            // safety:
            //
            // We have called `Vec::reserve_exact` to ensure the vec have
            // at least `*nread` bytes of unused memory.
            let mut read_buf = ReadBuf::uninit(unsafe { from_raw_parts_mut(ptr.add(len), *nread) });
            ready!(Pin::new(&mut *reader).poll_read(cx, &mut read_buf))?;

            let filled = read_buf.filled().len();

            // safety:
            //
            // `read_buf.filled().len()` return number of bytes read in.
            unsafe { vec.set_len(len + filled) };
            *nread -= filled;
        }

        Poll::Ready(Ok(()))
    }
}

/// * `nread` - bytes to read in
///
/// NOTE that this function does not modify any existing data.
pub fn read_exact_to_vec<'a, T: AsyncRead + ?Sized + Unpin>(
    reader: &'a mut T,
    vec: &'a mut Vec<u8>,
    nread: usize,
) -> ReadExactToVecFuture<'a, T> {
    vec.reserve_exact(nread);

    ReadExactToVecFuture { reader, vec, nread }
}

/// Returned future of [`read_exact_to_vec`].
#[derive(Debug)]
#[cfg(feature = "read-exact-to-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
pub struct ReadExactToBytesFuture<'a, T: ?Sized> {
    reader: &'a mut T,
    bytes: &'a mut bytes::BytesMut,
    nread: usize,
}

#[cfg(feature = "read-exact-to-bytes")]
impl<T: AsyncRead + ?Sized + Unpin> Future for ReadExactToBytesFuture<'_, T> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        use bytes::BufMut;

        let this = &mut *self;

        let reader = &mut *this.reader;
        let bytes = &mut *this.bytes;
        let nread = &mut this.nread;

        while *nread > 0 {
            let uninit_slice = bytes.chunk_mut();
            let len = std::cmp::min(uninit_slice.len(), *nread);

            // We have reserved space, so len shall not be 0.
            debug_assert_ne!(len, 0);

            // safety:
            //
            // `UninitSlice` is a transparent newtype over `[MaybeUninit<u8>]`.
            let uninit_slice: &mut [MaybeUninit<u8>] = unsafe { std::mem::transmute(uninit_slice) };

            let mut read_buf = ReadBuf::uninit(&mut uninit_slice[..len]);
            ready!(Pin::new(&mut *reader).poll_read(cx, &mut read_buf))?;

            let filled = read_buf.filled().len();

            unsafe { bytes.advance_mut(filled) };
            *nread -= filled;
        }

        Poll::Ready(Ok(()))
    }
}

#[cfg(feature = "read-exact-to-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
/// * `nread` - bytes to read in
///
/// NOTE that this function does not modify any existing data.
pub fn read_exact_to_bytes<'a, T: AsyncRead + ?Sized + Unpin>(
    reader: &'a mut T,
    bytes: &'a mut bytes::BytesMut,
    nread: usize,
) -> ReadExactToBytesFuture<'a, T> {
    bytes.reserve(nread);

    ReadExactToBytesFuture {
        reader,
        bytes,
        nread,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::AsyncWriteExt;

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
