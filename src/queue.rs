use std::cell::UnsafeCell;
use std::future::Future;
use std::io::{self, IoSlice};
use std::mem::{size_of, transmute, MaybeUninit};
use std::sync::atomic::{AtomicU16, Ordering};

use bytes::{Buf, Bytes};
use parking_lot::Mutex;

#[derive(Debug)]
pub struct MpScBytesQueue {
    bytes_queue: Box<[UnsafeCell<Bytes>]>,
    io_slice_buf: Mutex<Box<[u8]>>,

    /// The head to read from
    head: AtomicU16,

    /// The tail to write to.
    tail_pending: AtomicU16,

    /// The tail where writing is done.
    tail_done: AtomicU16,
}

impl MpScBytesQueue {
    pub fn new(cap: u16) -> Self {
        let bytes_queue: Vec<_> = (0..cap).map(|_| UnsafeCell::new(Bytes::new())).collect();
        let io_slice_buf: Vec<u8> = (0..(cap as usize) * size_of::<IoSlice>())
            .map(|_| 0)
            .collect();

        Self {
            bytes_queue: bytes_queue.into_boxed_slice(),
            io_slice_buf: Mutex::new(io_slice_buf.into_boxed_slice()),

            head: AtomicU16::new(0),
            tail_pending: AtomicU16::new(0),
            tail_done: AtomicU16::new(0),
        }
    }

    pub fn push<'bytes>(&self, slice: &'bytes [Bytes]) -> Result<(), &'bytes [Bytes]> {
        let queue_cap = self.bytes_queue.len();

        // Update tail_pending
        let mut tail_pending = self.tail_pending.load(Ordering::Relaxed);
        let mut new_tail_pending;

        loop {
            let remaining = (self.head.load(Ordering::Relaxed) as usize + queue_cap
                - tail_pending as usize)
                % queue_cap;
            if slice.len() > queue_cap || remaining < slice.len() {
                return Err(slice);
            }

            new_tail_pending =
                u16::overflowing_add(tail_pending, slice.len() as u16).0 % (queue_cap as u16);

            match self.tail_pending.compare_exchange_weak(
                tail_pending,
                new_tail_pending,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(new_value) => tail_pending = new_value,
            }
        }

        // Acquire load to wait for writes to complete
        self.head.load(Ordering::Acquire);

        let queue_cap = queue_cap as u16;

        // Write the value
        for bytes in slice {
            let ptr = self.bytes_queue[tail_pending as usize].get();
            unsafe { ptr.replace(bytes.clone()) };

            tail_pending = u16::overflowing_add(tail_pending, 1).0 % queue_cap;
        }

        // Update tail_done to new_tail_pending with Release
        while self.tail_done.load(Ordering::Relaxed) != tail_pending {}
        self.tail_done.store(new_tail_pending, Ordering::Release);

        Ok(())
    }

    /// * `write_vectored` - Must be cancel safe. Upon cancel, no byte should be written.
    ///
    /// # Cancel Safety
    ///
    /// This function is cancel safe.
    ///
    /// Upon on cancel, the internal buffer will only contain data not yet flushed.
    pub async fn pop_all_and_write_vectored<F, Ret>(&self, mut write_vectored: F) -> io::Result<()>
    where
        F: FnMut(&[IoSlice]) -> Ret,
        Ret: Future<Output = io::Result<usize>>,
    {
        let queue_cap = self.bytes_queue.len() as u16;

        let head = self.head.load(Ordering::Relaxed);
        // Acquire load to wait for writes to complete
        let tail = self.tail_done.load(Ordering::Acquire);

        if head == tail {
            // nothing to write
            return Ok(());
        }

        let mut guard = if let Some(guard) = self.io_slice_buf.try_lock() {
            guard
        } else {
            // Another thread is doing the write.
            return Ok(());
        };

        let pointer = &mut **guard as *mut [u8] as *mut [MaybeUninit<IoSlice>];
        let uninit_slice = unsafe { &mut *pointer };

        let mut i = 0;
        let mut j = head as usize;
        let tail = tail as usize;
        while j != tail {
            uninit_slice[i].write(IoSlice::new(unsafe { &**self.bytes_queue[j].get() }));
            j = usize::overflowing_add(j, 1).0 % (queue_cap as usize);
            i += 1;
        }

        let mut bufs: &mut [IoSlice] = unsafe { transmute(&mut uninit_slice[0..i]) };
        let mut head = head;

        // Loop Invariant: bufs must not be empty
        'outer: loop {
            // n must be greater than 0
            let mut n = write_vectored(bufs).await?;

            while bufs[0].len() <= n {
                // Update n and shrink bufs
                n -= bufs[0].len();
                bufs = &mut bufs[1..];

                // Increment head
                head = u16::overflowing_add(head, 1).0 % queue_cap;
                self.head.store(head, Ordering::Release);

                if bufs.is_empty() {
                    debug_assert_eq!(head as usize, tail);
                    return Ok(());
                }

                if n == 0 {
                    continue 'outer;
                }
            }

            let bytes = unsafe { &mut *self.bytes_queue[head as usize].get() };
            bytes.advance(n);
            bufs[0] = IoSlice::new(bytes);
        }
    }
}
