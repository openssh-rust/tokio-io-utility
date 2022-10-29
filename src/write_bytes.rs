use std::{io, pin::Pin};

use bytes::Bytes;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::{init_maybeuninit_io_slices_mut, ReusableIoSlices};

/// * `buffer` - must not contain empty `Bytes`s.
#[cfg_attr(docsrs, doc(cfg(feature = "bytes")))]
pub async fn write_all_bytes(
    mut writer: Pin<&mut (dyn AsyncWrite + Send)>,
    buffer: &mut Vec<Bytes>,
    reusable_io_slices: &mut ReusableIoSlices,
) -> io::Result<()> {
    static EMPTY_BYTES: Bytes = Bytes::from_static(b"");

    // `buffer` does not contain any empty `Bytes`s, so:
    //  - We can check for `io::ErrorKind::WriteZero` error easily
    //  - It won't occupy precise slot in `reusable_io_slices` so that
    //    we can group as many non-zero IoSlice in one write.
    //  - Avoid conserion from/to `VecDeque` unless necessary,
    //    which might allocate.
    //  - Simplify the loop below.

    if buffer.is_empty() {
        return Ok(());
    }

    let mut start = 0;

    // do-while style loop, because on the first iteration
    // start < buffer.len()
    'outer: loop {
        let uninit_io_slices = reusable_io_slices.get_mut();

        // start < buffer.len()
        // io_slices.is_empty() == false
        let io_slices = init_maybeuninit_io_slices_mut(
            uninit_io_slices,
            buffer[start..].iter().map(|bytes| io::IoSlice::new(bytes)),
        );

        let mut n = writer.write_vectored(io_slices).await?;

        if n == 0 {
            return Err(io::Error::from(io::ErrorKind::WriteZero));
        }

        // On first iteration, start < buffer.len()
        while n >= buffer[start].len() {
            n -= buffer[start].len();
            // Release `Bytes` so that the memory they occupied
            // can be reused in `BytesMut`.
            buffer[start] = EMPTY_BYTES.clone();
            start += 1;

            if start == buffer.len() {
                debug_assert_eq!(n, 0);
                break 'outer;
            }
        }

        // start < buffer.len(),
        // n < buffer[start].len()
        buffer[start] = buffer[start].slice(n..);
    }

    buffer.clear();

    Ok(())
}
