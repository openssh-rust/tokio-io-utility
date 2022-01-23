use std::cmp::min;
use std::collections::VecDeque;
use std::io::IoSlice;
use std::mem::{transmute, MaybeUninit};
use std::num::NonZeroUsize;
use std::slice::from_raw_parts_mut;

use bytes::{Buf, Bytes};
use parking_lot::{Mutex, MutexGuard};

/// Unbounded mpsc [`Bytes`] queue designed for grouping writes into one vectored write.
#[derive(Debug)]
pub struct MpScBytesQueue {
    bytes_queue: Mutex<VecDeque<Bytes>>,
    io_slice_buf: Mutex<Box<[MaybeUninit<IoSlice<'static>>]>>,
}

unsafe impl Send for MpScBytesQueue {}
unsafe impl Sync for MpScBytesQueue {}

impl MpScBytesQueue {
    /// * `cap` - This is the maximum amount of `io_slice`s that `Buffers::get_io_slices()`
    /// can return.
    ///
    /// Creates an empty queue with space for at least `cap` amount of elements.
    pub fn new(cap: NonZeroUsize) -> Self {
        let cap = cap.get();

        let bytes_queue = VecDeque::with_capacity(cap);
        let io_slice_buf: Vec<_> = (0..cap).map(|_| MaybeUninit::uninit()).collect();

        Self {
            bytes_queue: Mutex::new(bytes_queue),
            io_slice_buf: Mutex::new(io_slice_buf.into_boxed_slice()),
        }
    }

    pub fn capacity(&self) -> usize {
        self.bytes_queue.lock().capacity()
    }

    pub fn get_pusher(&self) -> QueuePusher<'_> {
        QueuePusher(self.bytes_queue.lock())
    }

    pub fn push(&self, bytes: Bytes) {
        self.get_pusher().push(bytes)
    }

    pub fn extend<const N: usize>(&self, bytes_array: [Bytes; N]) {
        self.get_pusher().extend(bytes_array)
    }

    // TODO: Add extend_from_iter, reserve, reserve_exact

    /// Return all buffers that need to be flushed.
    ///
    /// Return `None` if there isn't any buffer to flush or another
    /// thread is doing the flushing.
    pub fn get_buffers(&self) -> Option<Buffers<'_>> {
        let mut io_slices_guard = self.io_slice_buf.try_lock()?;

        let bytes_queue_guard = self.bytes_queue.lock();

        let len = bytes_queue_guard.len();
        if len == 0 {
            return None;
        }

        let io_slice_buf_len = io_slices_guard.len();
        let io_slice_buf_ptr = io_slices_guard.as_mut_ptr() as *mut u8 as *mut MaybeUninit<IoSlice>;

        // safety: This conversion reuses the memory of `io_slice_buf`.
        let uninit_slices = unsafe { from_raw_parts_mut(io_slice_buf_ptr, io_slice_buf_len) };

        bytes_queue_guard
            .iter()
            .zip(uninit_slices.iter_mut())
            .for_each(|(bytes, uninit_slice)| {
                uninit_slice.write(IoSlice::new(bytes));
            });

        Some(Buffers {
            queue: self,
            io_slices_guard,
            io_slice_start: 0,
            io_slice_end: min(len, io_slice_buf_len),
        })
    }
}

/// QueuePusher holds the lock, thus it is guaranteed that
/// all bytes pushed/extended will be inserted into the queue
/// in the same order push/extend is called and the `bytes_array`'s original order.
#[derive(Debug)]
pub struct QueuePusher<'a>(MutexGuard<'a, VecDeque<Bytes>>);

impl QueuePusher<'_> {
    pub fn push(&mut self, bytes: Bytes) {
        self.0.push_back(bytes);
    }

    pub fn extend<const N: usize>(&mut self, bytes_array: [Bytes; N]) {
        self.0.reserve_exact(N);
        self.0.extend(bytes_array);
    }

    pub fn reserve(&mut self, len: usize) {
        self.0.reserve(len);
    }

    pub fn reserve_exact(&mut self, len: usize) {
        self.0.reserve_exact(len);
    }

    pub fn extend_from_iter(&mut self, iter: impl IntoIterator<Item = Bytes>) {
        self.0.extend(iter);
    }
}

#[derive(Debug)]
pub struct Buffers<'a> {
    queue: &'a MpScBytesQueue,

    io_slices_guard: MutexGuard<'a, Box<[MaybeUninit<IoSlice<'static>>]>>,
    io_slice_start: usize,
    io_slice_end: usize,
}

impl Buffers<'_> {
    pub fn get_io_slices<'this>(&'this self) -> &[IoSlice<'this>] {
        let pointer = (&**self.io_slices_guard) as *const [MaybeUninit<IoSlice<'this>>];
        let uninit_slices: &[MaybeUninit<IoSlice>] = unsafe { &*pointer };

        // Safety: The io_slices are valid as long as the `MutexGuard` since there can only be one
        // consumer.
        unsafe { transmute(&uninit_slices[self.io_slice_start..self.io_slice_end]) }
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

        let io_slice_buf_len = self.io_slices_guard.len();
        let io_slice_buf_ptr =
            self.io_slices_guard.as_mut_ptr() as *mut u8 as *mut MaybeUninit<IoSlice>;

        // Safety: The io_slices are valid as long as the `MutexGuard` since there can only be one
        // consumer.
        let uninit_slices = unsafe { from_raw_parts_mut(io_slice_buf_ptr, io_slice_buf_len) };

        let mut bufs: &mut [IoSlice] =
            unsafe { transmute(&mut uninit_slices[self.io_slice_start..self.io_slice_end]) };

        if bufs.is_empty() {
            debug_assert_eq!(self.io_slice_start, self.io_slice_end);
            return false;
        }

        let mut bytes_queue_guard = queue.bytes_queue.lock();

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

        return true;
    }
}

#[cfg(test)]
mod tests {
    use super::MpScBytesQueue;

    use bytes::Bytes;
    use std::num::NonZeroUsize;

    use rayon::prelude::*;

    #[test]
    fn test_seq() {
        let bytes = Bytes::from_static(b"Hello, world!");

        let queue = MpScBytesQueue::new(NonZeroUsize::new(10).unwrap());

        for _ in 0..20 {
            assert!(queue.get_buffers().is_none());

            for i in 0..5 {
                eprintln!("Pushing (success) {}", i);
                queue.extend([bytes.clone(), bytes.clone()]);

                assert_eq!(
                    queue.get_buffers().unwrap().get_io_slices().len(),
                    (i + 1) * 2
                );
            }

            eprintln!("Test get_buffers");

            let bytes_slice_inserted = 10;

            let mut buffers = queue.get_buffers().unwrap();
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
        const BYTES0: Bytes = Bytes::from_static(b"012344578");
        const BYTES1: Bytes = Bytes::from_static(b"2134i9054");

        let queue = MpScBytesQueue::new(NonZeroUsize::new(1000).unwrap());

        rayon::scope(|s| {
            (0..1000).into_par_iter().for_each(|_| {
                s.spawn(|_| {
                    queue.extend([BYTES0, BYTES1]);
                });
            });

            let mut slices_processed = 0;
            loop {
                if let Some(mut buffers) = queue.get_buffers() {
                    let io_slices_len = {
                        let io_slices = buffers.get_io_slices();

                        // verify the content
                        let mut it = io_slices.into_iter();
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
}
