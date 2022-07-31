use super::ready;

use std::future::Future;
use std::io::{Error, ErrorKind, Result};
use std::marker::Unpin;
use std::mem::MaybeUninit;
use std::ops::Bound::*;
use std::pin::Pin;
use std::slice::from_raw_parts_mut;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, ReadBuf};

/// Returned future of [`read_to_vec`].
#[derive(Debug)]
pub struct ReadToVecFuture<'a, T: ?Sized> {
    reader: &'a mut T,
    vec: &'a mut Vec<u8>,
}
impl<T: AsyncRead + ?Sized + Unpin> Future for ReadToVecFuture<'_, T> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        let reader = &mut *this.reader;
        let vec = &mut *this.vec;

        let len = vec.len();
        let spare = vec.spare_capacity_mut();

        if !spare.is_empty() {
            // safety:
            //
            // nread is less than vec.capacity().
            let mut read_buf = ReadBuf::uninit(spare);
            ready!(Pin::new(&mut *reader).poll_read(cx, &mut read_buf))?;

            let filled = read_buf.filled().len();
            if filled == 0 {
                return Poll::Ready(Err(Error::new(
                    ErrorKind::UnexpectedEof,
                    "Unexpected Eof in ReadToVecFuture",
                )));
            }

            // safety:
            //
            // `read_buf.filled().len()` return number of bytes read in.
            unsafe { vec.set_len(len + filled) };
        }

        Poll::Ready(Ok(()))
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
    ReadToVecFuture { reader, vec }
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
    vec.reserve_exact(nread);

    ReadExactToVecFuture(read_to_vec_rng(reader, vec, nread..=nread))
}

#[derive(Debug)]
pub struct ReadToVecRngFuture<'a, T: ?Sized> {
    reader: &'a mut T,
    vec: &'a mut Vec<u8>,
    min: usize,
    max: usize,
}
impl<T: AsyncRead + ?Sized + Unpin> Future for ReadToVecRngFuture<'_, T> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        let reader = &mut *this.reader;
        let vec = &mut *this.vec;
        let min = &mut this.min;
        let max = &mut this.max;

        while *min > 0 {
            let ptr = vec.as_mut_ptr() as *mut MaybeUninit<u8>;
            let len = vec.len();

            // safety:
            //
            // The vec has at least *max bytes of unused memory.
            let mut read_buf = ReadBuf::uninit(unsafe { from_raw_parts_mut(ptr.add(len), *max) });
            ready!(Pin::new(&mut *reader).poll_read(cx, &mut read_buf))?;

            let filled = read_buf.filled().len();
            if filled == 0 {
                return Poll::Ready(Err(Error::new(
                    ErrorKind::UnexpectedEof,
                    "Unexpected Eof in ReadToVecFuture",
                )));
            }

            // safety:
            //
            // `read_buf.filled().len()` return number of bytes read in.
            unsafe { vec.set_len(len + filled) };
            *min -= filled;
            *max -= filled;
        }

        Poll::Ready(Ok(()))
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
    let min = match rng.start_bound().cloned() {
        Included(val) => val,
        Excluded(val) => val + 1,
        Unbounded => 0,
    };
    let max = match rng.end_bound().cloned() {
        Included(val) => val,
        Excluded(val) => val - 1,
        Unbounded => vec.capacity(),
    };
    vec.reserve(max);

    ReadToVecRngFuture {
        reader,
        vec,
        min,
        max,
    }
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
pub struct ReadToBytesRngFuture<'a, T: ?Sized> {
    reader: &'a mut T,
    bytes: &'a mut bytes::BytesMut,
    min: usize,
    max: usize,
}

#[cfg(feature = "read-exact-to-bytes")]
impl<T: AsyncRead + ?Sized + Unpin> Future for ReadToBytesRngFuture<'_, T> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        use bytes::BufMut;

        let this = &mut *self;

        let reader = &mut *this.reader;
        let bytes = &mut *this.bytes;
        let min = &mut this.min;
        let max = &mut this.max;

        while *min > 0 {
            let uninit_slice = bytes.chunk_mut();
            let len = std::cmp::min(uninit_slice.len(), *max);

            // We have reserved space, so len shall not be 0.
            debug_assert_ne!(len, 0);

            // safety:
            //
            // `UninitSlice` is a transparent newtype over `[MaybeUninit<u8>]`.
            let uninit_slice: &mut [MaybeUninit<u8>] = unsafe { std::mem::transmute(uninit_slice) };

            let mut read_buf = ReadBuf::uninit(&mut uninit_slice[..len]);
            ready!(Pin::new(&mut *reader).poll_read(cx, &mut read_buf))?;

            let filled = read_buf.filled().len();
            if filled == 0 {
                return Poll::Ready(Err(Error::new(
                    ErrorKind::UnexpectedEof,
                    "Unexpected Eof in ReadToVecFuture",
                )));
            }

            unsafe { bytes.advance_mut(filled) };

            *min = min.saturating_sub(filled);
            *max -= filled;
        }

        Poll::Ready(Ok(()))
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
    use std::ops::Bound::*;

    let min = match rng.start_bound().cloned() {
        Included(val) => val,
        Excluded(val) => val + 1,
        Unbounded => 0,
    };
    let max = match rng.end_bound().cloned() {
        Included(val) => val,
        Excluded(val) => val - 1,
        Unbounded => bytes.capacity(),
    };
    bytes.reserve(max);

    ReadToBytesRngFuture {
        reader,
        bytes,
        min,
        max,
    }
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
