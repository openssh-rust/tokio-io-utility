use std::{io, mem, pin::Pin, vec::IntoIter};

use bytes::Bytes;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::{init_maybeuninit_io_slices_mut, ReusableIoSlices};

/// * `buffer` - must not contain empty `Bytes`s.
#[cfg_attr(docsrs, doc(cfg(feature = "bytes")))]
pub async fn write_all_bytes(
    writer: Pin<&mut (dyn AsyncWrite + Send)>,
    buffer: &mut Vec<Bytes>,
    reusable_io_slices: &mut ReusableIoSlices,
) -> io::Result<()> {
    // `buffer` does not contain any empty `Bytes`s, so:
    //  - We can check for `io::ErrorKind::WriteZero` error easily
    //  - It won't occupy slots in `reusable_io_slices` so that
    //    we can group as many non-zero IoSlice in one write.
    //  - Avoid conserion from/to `VecDeque` unless necessary,
    //    which might allocate.
    //  - Simplify the loop in write_all_bytes_inner.

    if buffer.is_empty() {
        return Ok(());
    }

    // This is O(1)
    let mut iter = mem::take(buffer).into_iter();

    let res = write_all_bytes_inner(writer, &mut iter, reusable_io_slices).await;

    // This is O(1) because of the specailization in std
    *buffer = Vec::from_iter(iter);

    res
}

/// * `buffer` - contains at least one element and must not contain empty
///   `Bytes`
async fn write_all_bytes_inner(
    mut writer: Pin<&mut (dyn AsyncWrite + Send)>,
    iter: &mut IntoIter<Bytes>,
    reusable_io_slices: &mut ReusableIoSlices,
) -> io::Result<()> {
    // do-while style loop, because on the first iteration
    // iter must not be empty
    'outer: loop {
        let uninit_io_slices = reusable_io_slices.get_mut();

        // iter must not be empty
        // io_slices.is_empty() == false since uninit_io_slices also must not
        // be empty
        let io_slices = init_maybeuninit_io_slices_mut(
            uninit_io_slices,
            // Do not consume the iter yet since write_vectored might
            // do partial write.
            iter.as_slice().iter().map(|bytes| io::IoSlice::new(bytes)),
        );

        debug_assert!(!io_slices.is_empty());

        let mut n = writer.write_vectored(io_slices).await?;

        if n == 0 {
            // Since io_slices is not empty and it does not contain empty
            // `Bytes`, it must be WriteZero error.
            return Err(io::Error::from(io::ErrorKind::WriteZero));
        }

        // On first iteration, iter cannot be empty
        while n >= iter.as_slice()[0].len() {
            n -= iter.as_slice()[0].len();

            // Release `Bytes` so that the memory they occupied
            // can be reused in `BytesMut`.
            iter.next().unwrap();

            if iter.as_slice().is_empty() {
                debug_assert_eq!(n, 0);
                break 'outer;
            }
        }

        if n != 0 {
            // iter must not be empty
            let first = &mut iter.as_mut_slice()[0];
            // n < buffer[start].len()
            *first = first.slice(n..);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        iter,
        num::NonZeroUsize,
        task::{Context, Poll},
    };

    use bytes::BytesMut;
    use tokio::io::AsyncWrite;

    use super::*;
    use crate::IoSliceExt;

    /// Limit number of bytes that can be sent for each write.
    struct WriterRateLimit(usize, Vec<u8>);

    impl AsyncWrite for WriterRateLimit {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            let n = buf.len().min(self.0);
            let buf = &buf[..n];

            Pin::new(&mut self.1).poll_write(cx, buf)
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.1).poll_flush(cx)
        }
        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.1).poll_shutdown(cx)
        }

        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            bufs: &[io::IoSlice<'_>],
        ) -> Poll<io::Result<usize>> {
            let mut cnt = 0;
            let n = self.0;
            bufs.iter()
                .copied()
                .filter_map(|io_slice| {
                    (n > cnt).then(|| {
                        let n = io_slice.len().min(n - cnt);
                        cnt += n;
                        &io_slice.into_inner()[..n]
                    })
                })
                .for_each(|slice| {
                    self.1.extend(slice);
                });

            Poll::Ready(Ok(cnt))
        }

        fn is_write_vectored(&self) -> bool {
            true
        }
    }

    #[test]
    fn test() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let bytes: BytesMut = (0..255).collect();
                let bytes = bytes.freeze();

                let iter = iter::once(bytes).cycle().take(20);

                let expected_bytes: Vec<u8> = iter.clone().flatten().collect();

                let mut reusable_io_slices = ReusableIoSlices::new(NonZeroUsize::new(3).unwrap());

                // Emulate a pipe where each time only half of the Bytes can be
                // written.
                let writer = WriterRateLimit(255 / 2, Vec::new());
                tokio::pin!(writer);

                write_all_bytes(
                    writer.as_mut(),
                    &mut iter.clone().collect(),
                    &mut reusable_io_slices,
                )
                .await
                .unwrap();

                assert_eq!(writer.1, expected_bytes);

                // Emulate a pipe where each time exactly one Bytes can be
                // written.
                writer.0 = 255;
                writer.1.clear();

                write_all_bytes(
                    writer.as_mut(),
                    &mut iter.clone().collect(),
                    &mut reusable_io_slices,
                )
                .await
                .unwrap();

                assert_eq!(writer.1, expected_bytes);

                // Emulate a pipe where each time one and a half Bytes can be
                // written.
                writer.0 = 255 + 255 / 2;
                writer.1.clear();

                write_all_bytes(
                    writer.as_mut(),
                    &mut iter.clone().collect(),
                    &mut reusable_io_slices,
                )
                .await
                .unwrap();

                assert_eq!(writer.1, expected_bytes);

                // Emulate a pipe where each time one Bytes and a little bit
                // of the next Bytes can be written.
                writer.0 = 255 + 5;
                writer.1.clear();

                write_all_bytes(
                    writer.as_mut(),
                    &mut iter.clone().collect(),
                    &mut reusable_io_slices,
                )
                .await
                .unwrap();

                assert_eq!(writer.1, expected_bytes);
            });
    }
}
