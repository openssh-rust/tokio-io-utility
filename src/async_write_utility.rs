use core::future::Future;
use core::marker::Sized;
use core::pin::Pin;
use core::slice;
use core::task::{Context, Poll};

use std::io::{IoSlice, Result};
use tokio::io::AsyncWrite;

pub struct WriteVectorizedAll<'a, 'b, 'c, T: AsyncWriteUtility + ?Sized>(
    Pin<&'a mut T>,
    Option<&'b mut [IoSlice<'c>]>,
);

impl<T: AsyncWriteUtility + ?Sized> Future for WriteVectorizedAll<'_, '_, '_, T> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let slices = self.1.take().unwrap();

        match AsyncWriteUtility::poll_write_vectored_all(self.0.as_mut(), cx, slices) {
            Poll::Ready(res) => Poll::Ready(res),
            Poll::Pending => {
                self.1 = Some(slices);
                Poll::Pending
            }
        }
    }
}

pub trait AsyncWriteUtility: AsyncWrite {
    fn poll_write_vectored_all(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut bufs: &mut [IoSlice<'_>],
    ) -> Poll<Result<()>> {
        if bufs.is_empty() {
            return Poll::Ready(Ok(()));
        }

        // Loop Invariant: bufs must not be empty
        'outer: loop {
            // bytes must be greater than 0
            let mut bytes = match self.as_mut().poll_write_vectored(cx, bufs) {
                Poll::Ready(res) => res?,
                Poll::Pending => return Poll::Pending,
            };

            while bufs[0].len() <= bytes {
                bytes -= bufs[0].len();
                bufs = &mut bufs[1..];

                if bufs.is_empty() {
                    return Poll::Ready(Ok(()));
                }

                if bytes == 0 {
                    continue 'outer;
                }
            }

            let buf = &bufs[0][bytes..];
            bufs[0] = IoSlice::new(unsafe { slice::from_raw_parts(buf.as_ptr(), buf.len()) });
        }
    }

    /// Equivalent to:
    ///
    /// ```ignore
    /// async fn write_vectored_all(&mut self, bufs: &mut [IoSlice<'_>]) -> Result<()>;
    /// ```
    fn write_vectored_all<'a, 'b, 'c>(
        self: Pin<&'a mut Self>,
        bufs: &'b mut [IoSlice<'c>],
    ) -> WriteVectorizedAll<'a, 'b, 'c, Self> {
        WriteVectorizedAll(self, Some(bufs))
    }
}

impl<T: AsyncWrite + ?Sized> AsyncWriteUtility for T {}

#[cfg(test)]
mod tests {
    use super::AsyncWriteUtility;

    use core::pin::Pin;

    use std::io::IoSlice;
    use std::slice::from_raw_parts;
    use tokio::io::AsyncReadExt;

    fn as_ioslice<T>(v: &[T]) -> IoSlice<'_> {
        IoSlice::new(unsafe {
            from_raw_parts(v.as_ptr() as *const u8, v.len() * std::mem::size_of::<T>())
        })
    }

    #[test]
    fn test() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (mut r, mut w) = tokio_pipe::pipe().unwrap();

                let w_task = tokio::spawn(async move {
                    let buffer: Vec<u32> = (0..1024).collect();
                    let pinned = Pin::new(&mut w);
                    pinned
                        .write_vectored_all(&mut [as_ioslice(&buffer), as_ioslice(&buffer)])
                        .await
                        .unwrap();
                });

                let r_task = tokio::spawn(async move {
                    let mut n = 0u32;
                    let mut buf = [0; 4 * 128];
                    while n < 1024 {
                        r.read_exact(&mut buf).await.unwrap();
                        for x in buf.chunks(4) {
                            assert_eq!(x, n.to_ne_bytes());
                            n += 1;
                        }
                    }

                    n = 0;
                    while n < 1024 {
                        r.read_exact(&mut buf).await.unwrap();
                        for x in buf.chunks(4) {
                            assert_eq!(x, n.to_ne_bytes());
                            n += 1;
                        }
                    }
                });
                r_task.await.unwrap();
                w_task.await.unwrap();
            });
    }
}
