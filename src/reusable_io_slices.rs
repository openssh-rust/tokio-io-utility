use std::{
    io::IoSlice, mem::MaybeUninit, num::NonZeroUsize, ptr::NonNull, slice::from_raw_parts_mut,
};

#[derive(Debug)]
pub struct ReusableIoSlices {
    ptr: NonNull<()>,
    cap: NonZeroUsize,
}

unsafe impl Send for ReusableIoSlices {}
unsafe impl Sync for ReusableIoSlices {}

impl ReusableIoSlices {
    /// Create new [`ReusableIoSlices`].
    pub fn new(cap: NonZeroUsize) -> Self {
        let io_slices: Vec<MaybeUninit<IoSlice<'_>>> =
            (0..cap.get()).map(|_| MaybeUninit::uninit()).collect();

        let io_slices: Box<_> = io_slices.into_boxed_slice();

        let io_slices = Box::into_raw(io_slices);

        // Safety:
        //
        // io_slices is result of `Box::into_raw`
        let io_slices: &mut [MaybeUninit<IoSlice<'_>>] = unsafe { &mut *io_slices };

        let ptr = io_slices.as_mut_ptr();

        // Safety:
        //
        // io_slices is a valid allocation, thus slice::as_mut_ptr
        // must return a non-null pointer
        let ptr = unsafe { NonNull::new_unchecked(ptr as *mut ()) };

        debug_assert_eq!(io_slices.len(), cap.get());

        Self { ptr, cap }
    }

    pub fn get(&mut self) -> &mut [MaybeUninit<IoSlice<'_>>] {
        // Safety:
        //  - self.ptr.as_ptr() is result of io_slices.as_mut_ptr()
        //  - self.cap.get() == io_slices.len()
        unsafe {
            from_raw_parts_mut(
                self.ptr.as_ptr() as *mut MaybeUninit<IoSlice<'_>>,
                self.cap.get(),
            )
        }
    }
}

impl Drop for ReusableIoSlices {
    fn drop(&mut self) {
        let io_slices = self.get() as *mut _;
        // Safety:
        //
        // io_slices is created using `Box::into_raw`.
        drop(unsafe { Box::from_raw(io_slices) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reusable_io_slices() {
        let io_slice = IoSlice::new(b"123exr3x");

        for size in 1..300 {
            let cap = NonZeroUsize::new(size).unwrap();
            let mut reusable_io_slices = ReusableIoSlices::new(cap);

            for uninit_io_slice in reusable_io_slices.get() {
                uninit_io_slice.write(io_slice);
            }
        }
    }
}
