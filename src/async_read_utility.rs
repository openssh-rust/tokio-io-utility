use std::future::Future;
use std::io::Result;
use std::marker::Unpin;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::slice::from_raw_parts_mut;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

macro_rules! ready {
    ($e:expr) => {
        match $e {
            Poll::Ready(t) => t,
            Poll::Pending => return Poll::Pending,
        }
    };
}

struct PtrWrapper(*mut u8);
unsafe impl Send for PtrWrapper {}

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

#[cfg(feature = "read-exact-to-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
/// * `nread` - bytes to read in
///
/// NOTE that this function does not modify any existing data.
pub async fn read_exact_to_bytes<T: AsyncRead + ?Sized + Unpin>(
    reader: &mut T,
    bytes: &mut bytes::BytesMut,
    mut nread: usize,
) -> Result<()> {
    use bytes::BufMut;

    bytes.reserve(nread);

    while nread > 0 {
        let uninit_slice = bytes.chunk_mut();
        let len = std::cmp::min(uninit_slice.len(), nread);

        // safety:
        //
        // bytes.chunk_mut() return a &mut Uninit, which can be
        // converted into &mut [u8] except that it is uninitialized.
        //
        // `slice` here will not be read from, only write to.
        let slice = unsafe { from_raw_parts_mut(uninit_slice.as_mut_ptr(), len) };
        reader.read_exact(slice).await?;

        unsafe { bytes.advance_mut(len) };
        nread -= len;
    }

    Ok(())
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
