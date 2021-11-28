use core::pin::Pin;
use core::slice;
use core::task::{Context, Poll};

use std::io::{IoSlice, Result};
use tokio::io::AsyncWrite;

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
}
