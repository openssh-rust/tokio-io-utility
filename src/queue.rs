use std::cell::UnsafeCell;
use std::io::IoSlice;
use std::mem::{size_of, transmute, MaybeUninit};
use std::sync::atomic::{AtomicU16, Ordering};

use bytes::{Buf, Bytes};
use parking_lot::{Mutex, MutexGuard};

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

    pub fn capacity(&self) -> usize {
        self.bytes_queue.len()
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

    /// Return all buffers that need to be flushed.
    ///
    /// Return `None` if there isn't any buffer to flush or another
    /// thread is doing the flushing.
    pub async fn get_buffers<F, Ret>(&self) -> Option<Buffers<'_>> {
        let queue_cap = self.bytes_queue.len() as u16;

        let head = self.head.load(Ordering::Relaxed);
        // Acquire load to wait for writes to complete
        let tail = self.tail_done.load(Ordering::Acquire);

        if head == tail {
            // nothing to write
            return None;
        }

        let mut guard = self.io_slice_buf.try_lock()?;

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

        Some(Buffers {
            queue: self,
            guard,
            io_slice_start: 0,
            io_slice_end: i as u16,
            head,
            tail: tail as u16,
        })
    }
}

#[derive(Debug)]
pub struct Buffers<'a> {
    queue: &'a MpScBytesQueue,

    guard: MutexGuard<'a, Box<[u8]>>,
    io_slice_start: u16,
    io_slice_end: u16,
    head: u16,
    tail: u16,
}

impl Buffers<'_> {
    pub fn get_io_slices(&self) -> &[IoSlice] {
        let pointer = &**self.guard as *const [u8] as *const [MaybeUninit<IoSlice>];
        let uninit_slice = unsafe { &*pointer };
        unsafe {
            transmute(&uninit_slice[self.io_slice_start as usize..self.io_slice_end as usize])
        }
    }

    unsafe fn get_first_bytes(&mut self) -> &mut Bytes {
        &mut *self.queue.bytes_queue[self.head as usize].get()
    }

    /// * `n` - bytes successfully written.
    ///
    /// Return `true` if another iteration is required,
    /// `false` if the loop can terminate right away.
    pub fn advance(&mut self, mut n: usize) -> bool {
        let queue = self.queue;
        let queue_cap = queue.capacity() as u16;

        let pointer = &mut **self.guard as *mut [u8] as *mut [MaybeUninit<IoSlice>];
        let uninit_slice = unsafe { &mut *pointer };
        let mut bufs: &mut [IoSlice] = unsafe {
            transmute(&mut uninit_slice[self.io_slice_start as usize..self.io_slice_end as usize])
        };

        if bufs.is_empty() {
            return false;
        }

        while bufs[0].len() <= n {
            // Update n and shrink bufs
            n -= bufs[0].len();
            bufs = &mut bufs[1..];
            self.io_slice_start += 1;

            // Reset Bytes
            *unsafe { self.get_first_bytes() } = Bytes::new();

            // Increment head
            self.head = u16::overflowing_add(self.head, 1).0 % queue_cap;
            queue.head.store(self.head, Ordering::Release);

            if bufs.is_empty() {
                debug_assert_eq!(self.head, self.tail);
                return false;
            }

            if n == 0 {
                return true;
            }
        }

        let bytes = unsafe { self.get_first_bytes() };
        bytes.advance(n);
        bufs[0] = IoSlice::new(bytes);

        return true;
    }
}
