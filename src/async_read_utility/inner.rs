use std::{
    future::Future,
    io::{Error, ErrorKind, Result},
    marker::Unpin,
    mem::MaybeUninit,
    ops::Bound::*,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, ReadBuf};

pub trait Container {
    /// Reserve at least `n` bytes that can be used in
    /// [`Container::spare_mut`].
    fn reserve(&mut self, n: usize);

    /// Return the capacity reserved.
    fn capacity(&self) -> usize;

    /// The returned uninit slice must not be empty.
    ///
    /// NOTE that the returned uninit slice might be smaller
    /// than [`Container::capacity`] or bytes reserved in
    /// [`Container::reserve`].
    ///
    /// This is because that the container might be a ring buffer.
    /// If you consume all uninit slices, then the sum of their lengths
    /// must be equal to [`Container::capacity`].
    ///
    /// # Safety
    ///
    /// The slice returned must not be read from and users should
    /// never write uninitialized bytes to it.
    unsafe fn spare_mut(&mut self, max: usize) -> &mut [MaybeUninit<u8>];

    /// # Safety
    ///
    /// The users must have actually initialized at least `n` bytes
    /// in the uninit slice returned by [`Container::spare_mut`].
    unsafe fn advance(&mut self, n: usize);
}

impl<T: Container> Container for &mut T {
    fn reserve(&mut self, n: usize) {
        (*self).reserve(n)
    }

    fn capacity(&self) -> usize {
        (**self).capacity()
    }

    unsafe fn spare_mut(&mut self, max: usize) -> &mut [MaybeUninit<u8>] {
        (*self).spare_mut(max)
    }

    unsafe fn advance(&mut self, n: usize) {
        (*self).advance(n)
    }
}

impl Container for Vec<u8> {
    fn reserve(&mut self, n: usize) {
        Vec::reserve(self, n)
    }

    fn capacity(&self) -> usize {
        Vec::capacity(self)
    }

    unsafe fn spare_mut(&mut self, max: usize) -> &mut [MaybeUninit<u8>] {
        // The uninit slice must be at least as long as max
        &mut self.spare_capacity_mut()[..max]
    }

    unsafe fn advance(&mut self, n: usize) {
        let len = self.len();
        self.set_len(len + n)
    }
}

#[cfg(feature = "read-exact-to-bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "read-exact-to-bytes")))]
impl Container for bytes::BytesMut {
    fn reserve(&mut self, n: usize) {
        bytes::BytesMut::reserve(self, n)
    }

    fn capacity(&self) -> usize {
        bytes::BytesMut::capacity(self)
    }

    unsafe fn spare_mut(&mut self, max: usize) -> &mut [MaybeUninit<u8>] {
        use bytes::BufMut;

        let uninit_slice = self.chunk_mut().as_uninit_slice_mut();
        let len = std::cmp::min(uninit_slice.len(), max);
        &mut uninit_slice[..len]
    }

    unsafe fn advance(&mut self, n: usize) {
        use bytes::BufMut;

        self.advance_mut(n)
    }
}

#[derive(Debug)]
pub struct ReadToContainerRngFuture<'a, C: ?Sized, Reader: ?Sized> {
    reader: &'a mut Reader,
    container: &'a mut C,
    min: usize,
    max: usize,
}

impl<C, Reader> Future for ReadToContainerRngFuture<'_, C, Reader>
where
    C: Container + ?Sized,
    Reader: AsyncRead + ?Sized + Unpin,
{
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        let reader = &mut *this.reader;
        let container = &mut *this.container;
        let min = &mut this.min;
        let max = &mut this.max;

        if *max == 0 {
            return Poll::Ready(Ok(()));
        }

        // Do not test *min here so that if:
        //
        // ```rust
        // read_to_container_rng(r, c, 0..10).await
        // ```
        //
        // is called, then we would at least try toread in some bytes.
        loop {
            // safety:
            //
            // We will never read from it and never write uninitialized bytes
            // to it.
            let uninit_slice = unsafe { container.spare_mut(*max) };

            debug_assert_ne!(uninit_slice.len(), 0);

            let mut read_buf = ReadBuf::uninit(uninit_slice);
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
            unsafe { container.advance(filled) };

            *min = min.saturating_sub(filled);
            *max -= filled;

            if *min == 0 {
                break;
            }
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
///           The lower bound must not be larger than the upper bound.
///
/// Return [`ErrorKind::UnexpectedEof`] on Eof.
///
/// NOTE that this function does not modify any existing data.
///
/// # Cancel safety
///
/// It is cancel safe and dropping the returned future will not stop the
/// wakeup from happening.
pub fn read_to_container_rng<'a, C, Reader>(
    reader: &'a mut Reader,
    container: &'a mut C,
    rng: impl std::ops::RangeBounds<usize>,
) -> ReadToContainerRngFuture<'a, C, Reader>
where
    C: Container + ?Sized,
    Reader: AsyncRead + ?Sized + Unpin,
{
    let min = match rng.start_bound().cloned() {
        Included(val) => val,
        Excluded(val) => val + 1,
        Unbounded => 0,
    };
    let max = match rng.end_bound().cloned() {
        Included(val) => val,
        Excluded(val) => val - 1,
        Unbounded => container.capacity(),
    };
    container.reserve(max);

    assert!(min <= max, "min {min} should be no larger than max {max}");

    ReadToContainerRngFuture {
        reader,
        container,
        min,
        max,
    }
}
