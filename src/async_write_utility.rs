use core::slice;

use std::io::{self, IoSlice, Result};
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

pub async fn write_vectored_all<Writer: AsyncWrite + Unpin>(
    writer: &mut Writer,
    mut bufs: &mut [IoSlice<'_>],
) -> Result<()> {
    if bufs.is_empty() {
        return Ok(());
    }

    // Loop Invariant: bufs must not be empty
    'outer: loop {
        // bytes must be greater than 0
        let mut bytes = writer.write_vectored(bufs).await?;

        if bytes == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, ""));
        }

        while bufs[0].len() <= bytes {
            bytes -= bufs[0].len();
            bufs = &mut bufs[1..];

            if bufs.is_empty() {
                return Ok(());
            }

            if bytes == 0 {
                continue 'outer;
            }
        }

        let buf = &bufs[0][bytes..];
        bufs[0] = IoSlice::new(unsafe { slice::from_raw_parts(buf.as_ptr(), buf.len()) });
    }
}

#[cfg(test)]
mod tests {
    use super::write_vectored_all;

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
                    let buffer0: Vec<u32> = (0..1024).collect();
                    let buffer1: Vec<u32> = (1024..2048).collect();

                    write_vectored_all(&mut w, &mut [as_ioslice(&buffer0), as_ioslice(&buffer1)])
                        .await
                        .unwrap();

                    write_vectored_all(&mut w, &mut [as_ioslice(&buffer0), as_ioslice(&buffer1)])
                        .await
                        .unwrap();
                });

                let r_task = tokio::spawn(async move {
                    for _ in 0..2 {
                        let mut n = 0u32;
                        let mut buf = [0; 4 * 128];
                        while n < 2048 {
                            r.read_exact(&mut buf).await.unwrap();
                            for x in buf.chunks(4) {
                                assert_eq!(x, n.to_ne_bytes());
                                n += 1;
                            }
                        }
                    }
                });
                r_task.await.unwrap();
                w_task.await.unwrap();
            });
    }
}
