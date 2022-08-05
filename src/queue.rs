use std::{
    cmp::min,
    collections::VecDeque,
    io::IoSlice,
    iter::{ExactSizeIterator, Iterator},
    mem::MaybeUninit,
    num::NonZeroUsize,
};

pub use std::collections::vec_deque::Drain;

use bytes::{Buf, Bytes};
use parking_lot::{Mutex, MutexGuard};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard as AsyncMutexGuard};

use super::ReusableIoSlices;

/// Unbounded mpsc [`Bytes`] queue designed for grouping writes into one vectored write.
#[derive(Debug)]
pub struct MpScBytesQueue {
    bytes_queue: Mutex<VecDeque<Bytes>>,
    io_slice_buf: AsyncMutex<ReusableIoSlices>,
}

impl MpScBytesQueue {
    /// * `cap` - This is the maximum amount of `io_slice`s that `Buffers::get_io_slices()`
    /// can return.
    ///
    /// Creates an empty queue with space for at least `cap` amount of elements.
    pub fn new(cap: NonZeroUsize) -> Self {
        let bytes_queue = VecDeque::with_capacity(cap.get());

        Self {
            bytes_queue: Mutex::new(bytes_queue),
            io_slice_buf: AsyncMutex::new(ReusableIoSlices::new(cap)),
        }
    }

    pub fn capacity(&self) -> usize {
        self.bytes_queue.lock().capacity()
    }

    pub fn get_pusher(&self) -> QueuePusher<'_> {
        QueuePusher(self.bytes_queue.lock())
    }

    pub fn push(&self, bytes: Bytes) {
        if !bytes.is_empty() {
            self.get_pusher().push(bytes)
        }
    }

    pub fn extend<const N: usize>(&self, bytes_array: [Bytes; N]) {
        self.get_pusher().extend(bytes_array)
    }

    pub fn extend_from_iter(&self, iter: impl IntoIterator<Item = Bytes>) {
        self.get_pusher().extend_from_iter(iter)
    }

    pub fn extend_from_exact_size_iter<I, It>(&self, iter: I)
    where
        I: IntoIterator<Item = Bytes, IntoIter = It>,
        It: ExactSizeIterator + Iterator<Item = Bytes>,
    {
        self.get_pusher().extend_from_exact_size_iter(iter);
    }

    pub fn reserve(&self, len: usize) {
        self.get_pusher().reserve(len);
    }

    pub fn reserve_exact(&self, len: usize) {
        self.get_pusher().reserve_exact(len);
    }

    fn get_buffers_impl<'this>(
        &'this self,
        mut io_slices_guard: AsyncMutexGuard<'this, ReusableIoSlices>,
    ) -> Buffers<'this> {
        let bytes_queue_guard = self.bytes_queue.lock();

        let len = bytes_queue_guard.len();

        let uninit_slices = io_slices_guard.get_mut();
        let io_slice_buf_len = uninit_slices.len();

        bytes_queue_guard
            .iter()
            .zip(uninit_slices.iter_mut())
            .for_each(|(bytes, uninit_slice)| {
                // Every bytes is non-empty, so every io_slice created is non-empty.
                *uninit_slice = MaybeUninit::new(IoSlice::new(bytes));
            });

        Buffers {
            queue: self,
            io_slices_guard,
            io_slice_start: 0,
            io_slice_end: min(len, io_slice_buf_len),
        }
    }

    /// Return all buffers that need to be flushed.
    ///
    /// Return `None` if another thread is doing the flushing.
    pub fn try_get_buffers(&self) -> Option<Buffers<'_>> {
        if let Ok(guard) = self.io_slice_buf.try_lock() {
            Some(self.get_buffers_impl(guard))
        } else {
            None
        }
    }

    /// Return all buffers that need to be flushed.
    ///
    /// If another thread is doing the flushing, then
    /// wait until it is done.
    pub async fn get_buffers_blocked(&self) -> Buffers<'_> {
        self.get_buffers_impl(self.io_slice_buf.lock().await)
    }
}

/// QueuePusher holds the lock, thus it is guaranteed that
/// all bytes pushed/extended will be inserted into the queue
/// in the same order push/extend is called and the `bytes_array`'s original order.
#[derive(Debug)]
pub struct QueuePusher<'a>(MutexGuard<'a, VecDeque<Bytes>>);

impl QueuePusher<'_> {
    pub fn push(&mut self, bytes: Bytes) {
        if !bytes.is_empty() {
            self.0.push_back(bytes);
        }
    }

    fn extend_impl<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = Bytes>,
    {
        self.0
            .extend(iter.into_iter().filter(|bytes| !bytes.is_empty()));
    }

    pub fn extend<const N: usize>(&mut self, bytes_array: [Bytes; N]) {
        self.0.reserve_exact(N);
        self.extend_impl(bytes_array.iter().cloned());
    }

    pub fn extend_from_iter(&mut self, iter: impl IntoIterator<Item = Bytes>) {
        self.extend_impl(iter);
    }

    pub fn extend_from_exact_size_iter<I, It>(&mut self, iter: I)
    where
        I: IntoIterator<Item = Bytes, IntoIter = It>,
        It: ExactSizeIterator + Iterator<Item = Bytes>,
    {
        let iter = iter.into_iter();
        self.0.reserve_exact(iter.len());
        self.extend_impl(iter);
    }

    pub fn reserve(&mut self, len: usize) {
        self.0.reserve(len);
    }

    pub fn reserve_exact(&mut self, len: usize) {
        self.0.reserve_exact(len);
    }
}

/// Return `IoSlice`s suitable for writing.
#[derive(Debug)]
pub struct Buffers<'a> {
    queue: &'a MpScBytesQueue,

    io_slices_guard: AsyncMutexGuard<'a, ReusableIoSlices>,
    io_slice_start: usize,
    io_slice_end: usize,
}

impl<'a> Buffers<'a> {
    /// Return `IoSlice`s that every one of them is non-empty.
    pub fn get_io_slices<'this>(&'this self) -> &[IoSlice<'this>] {
        let uninit_slices = &self.io_slices_guard.get()[self.io_slice_start..self.io_slice_end];

        // Safety: The io_slices are valid as long as the `MutexGuard` since there can only be one
        // consumer.
        unsafe { &*(uninit_slices as *const _ as *const [IoSlice<'_>]) }
    }

    /// Return `true` if no `io_slices` is left.
    pub fn is_empty(&self) -> bool {
        self.io_slice_start == self.io_slice_end
    }

    /// Drain all [`Bytes`] stored in [`MpScBytesQueue`] so that they can move
    /// in a zero-copy manner.
    pub fn drain_bytes(self) -> DrainBytes<'a> {
        DrainBytes {
            _io_slices_guard: self.io_slices_guard,
            deque: self.queue.bytes_queue.lock(),
        }
    }

    /// * `n` - bytes successfully written.
    ///
    /// Return `true` if another iteration is required,
    /// `false` if the loop can terminate right away.
    ///
    /// After this function call, `MpScBytesQueue` will have `n` buffered
    /// bytes removed.
    pub fn advance(&mut self, n: NonZeroUsize) -> bool {
        let mut n = n.get();

        let queue = self.queue;

        let mut bufs: &mut [IoSlice<'_>] = {
            let uninit_slices =
                &mut self.io_slices_guard.get_mut()[self.io_slice_start..self.io_slice_end];

            // Safety: The io_slices are valid as long as the `MutexGuard` since there can only be one
            // consumer.
            unsafe { &mut *(uninit_slices as *mut _ as *mut [IoSlice<'_>]) }
        };

        if bufs.is_empty() {
            debug_assert_eq!(self.io_slice_start, self.io_slice_end);
            return false;
        }

        let mut bytes_queue_guard = queue.bytes_queue.lock();

        // Every bytes is non-empty, so every io_slice created is non-empty.
        while bufs[0].len() <= n {
            // Update n and shrink bufs
            n -= bufs[0].len();
            bufs = &mut bufs[1..];
            self.io_slice_start += 1;

            // Reset Bytes
            bytes_queue_guard.pop_front().unwrap();

            if bufs.is_empty() {
                debug_assert_eq!(self.io_slice_start, self.io_slice_end);
                return false;
            }

            if n == 0 {
                debug_assert_ne!(self.io_slice_start, self.io_slice_end);
                return true;
            }
        }

        let bytes = bytes_queue_guard.front_mut().unwrap();
        bytes.advance(n);
        bufs[0] = IoSlice::new(bytes);

        debug_assert_ne!(self.io_slice_start, self.io_slice_end);

        true
    }
}

/// Return bytes in the same order they are pushed.
///
/// This struct holds locks to [`MpScBytesQueue`], which prevents new [`Bytes`]
/// from being pushed, thus you shall not do any IO until this variable of this
/// type is dropped.
#[derive(Debug)]
pub struct DrainBytes<'a> {
    _io_slices_guard: AsyncMutexGuard<'a, ReusableIoSlices>,
    deque: MutexGuard<'a, VecDeque<Bytes>>,
}

impl Iterator for DrainBytes<'_> {
    type Item = Bytes;

    fn next(&mut self) -> Option<Bytes> {
        self.deque.pop_front()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.deque.len();

        (len, Some(len))
    }
}

impl ExactSizeIterator for DrainBytes<'_> {}

#[cfg(test)]
mod tests {
    use super::{Buffers, MpScBytesQueue};

    use bytes::Bytes;
    use std::num::NonZeroUsize;

    use rayon::prelude::*;

    fn assert_empty(buffers: Buffers<'_>) {
        assert!(buffers.is_empty());
        assert!(buffers.get_io_slices().is_empty());
    }

    #[test]
    fn test_seq() {
        let bytes = Bytes::from_static(b"Hello, world!");

        let queue = MpScBytesQueue::new(NonZeroUsize::new(10).unwrap());

        for _ in 0..20 {
            // Test extend
            assert_empty(queue.try_get_buffers().unwrap());

            for i in 0..5 {
                eprintln!("Pushing (success) {}", i);
                queue.extend([bytes.clone(), bytes.clone()]);

                assert_eq!(
                    queue.try_get_buffers().unwrap().get_io_slices().len(),
                    (i + 1) * 2
                );
            }

            eprintln!("Test try_get_buffers");

            let bytes_slice_inserted = 10;

            {
                let mut buffers = queue.try_get_buffers().unwrap();
                assert_eq!(buffers.get_io_slices().len(), bytes_slice_inserted);
                for io_slice in buffers.get_io_slices() {
                    assert_eq!(&**io_slice, &*bytes);
                }

                assert!(!buffers
                    .advance(NonZeroUsize::new(bytes_slice_inserted * bytes.len()).unwrap()));
                assert!(!buffers.advance(NonZeroUsize::new(100).unwrap()));
            }

            // Test push
            assert_empty(queue.try_get_buffers().unwrap());

            for i in 0..10 {
                eprintln!("Pushing (success) {}", i);
                queue.push(bytes.clone());

                assert_eq!(
                    queue.try_get_buffers().unwrap().get_io_slices().len(),
                    i + 1
                );
            }

            eprintln!("Test try_get_buffers");

            let bytes_slice_inserted = 10;

            let mut buffers = queue.try_get_buffers().unwrap();
            assert_eq!(buffers.get_io_slices().len(), bytes_slice_inserted);
            for io_slice in buffers.get_io_slices() {
                assert_eq!(&**io_slice, &*bytes);
            }

            assert!(
                !buffers.advance(NonZeroUsize::new(bytes_slice_inserted * bytes.len()).unwrap())
            );
            assert!(!buffers.advance(NonZeroUsize::new(100).unwrap()));
        }
    }

    #[test]
    fn test_par() {
        static BYTES0: Bytes = Bytes::from_static(b"012344578");
        static BYTES1: Bytes = Bytes::from_static(b"2134i9054");

        let queue = MpScBytesQueue::new(NonZeroUsize::new(1000).unwrap());

        rayon::scope(|s| {
            (0..1000).into_par_iter().for_each(|_| {
                s.spawn(|_| {
                    queue.extend([BYTES0.clone(), BYTES1.clone()]);
                });
            });

            let mut slices_processed = 0;
            loop {
                if let Some(mut buffers) = queue.try_get_buffers() {
                    if buffers.is_empty() {
                        continue;
                    }

                    let io_slices_len = {
                        let io_slices = buffers.get_io_slices();

                        // verify the content
                        let mut it = io_slices.iter();
                        while let Some(io_slice0) = it.next() {
                            assert_eq!(&**io_slice0, &*BYTES0);
                            assert_eq!(&**it.next().unwrap(), &*BYTES1);
                        }
                        io_slices.len()
                    };

                    // advance
                    buffers.advance(NonZeroUsize::new(io_slices_len * BYTES0.len()).unwrap());
                    slices_processed += io_slices_len;

                    if slices_processed == 2000 {
                        break;
                    }
                }
            }
        });
    }

    #[test]
    fn test_drain_bytes() {
        static BYTES0: Bytes = Bytes::from_static(b"012344578");
        static BYTES1: Bytes = Bytes::from_static(b"2134i9054");

        let queue = MpScBytesQueue::new(NonZeroUsize::new(1000).unwrap());

        for _ in 0..20 {
            // Test extend
            assert_empty(queue.try_get_buffers().unwrap());

            for i in 0..5 {
                eprintln!("Pushing (success) {}", i);
                queue.extend([BYTES0.clone(), BYTES1.clone()]);

                assert_eq!(
                    queue.try_get_buffers().unwrap().get_io_slices().len(),
                    (i + 1) * 2
                );
            }

            eprintln!("Test try_get_buffers");

            for (i, bytes) in queue.try_get_buffers().unwrap().drain_bytes().enumerate() {
                if i % 2 == 0 {
                    assert_eq!(bytes, BYTES0);
                } else {
                    assert_eq!(bytes, BYTES1);
                }
            }
        }
    }
}
