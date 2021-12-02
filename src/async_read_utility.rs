use core::marker::Unpin;
use core::slice::from_raw_parts_mut;

use std::io::Result;

use tokio::io::{AsyncRead, AsyncReadExt};

struct PtrWrapper(*mut u8);
unsafe impl Send for PtrWrapper {}

/// * `nread` - bytes to read in
///
/// NOTE that this function does not modify any existing data.
pub async fn read_exact_to_vec<T: AsyncRead + ?Sized + Unpin>(
    reader: &mut T,
    vec: &mut Vec<u8>,
    nread: usize,
) -> Result<()> {
    vec.reserve_exact(nread);

    let ptr = PtrWrapper(vec.as_mut_ptr());
    let len = vec.len();
    let cap = vec.capacity();

    {
        let slice = unsafe { from_raw_parts_mut(ptr.0.add(len), nread) };
        reader.read_exact(slice).await?;
    }

    unsafe { (vec as *mut Vec<u8>).write(Vec::from_raw_parts(ptr.0, len + nread, cap)) };

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::read_exact_to_vec;

    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn test() {
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
    }
}
