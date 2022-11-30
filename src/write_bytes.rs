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
    //  - It won't occupy precise slot in `reusable_io_slices` so that
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

        // iter must not be empty
        let first = &mut iter.as_mut_slice()[0];
        // n < buffer[start].len()
        *first = first.slice(n..);
    }

    Ok(())
}
