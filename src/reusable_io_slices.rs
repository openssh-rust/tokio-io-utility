use std::{
    io::IoSlice,
    mem::{ManuallyDrop, MaybeUninit},
    num::NonZeroUsize,
    ptr::NonNull,
    slice::from_raw_parts_mut,
};

/// [`Box`]ed [`IoSlice`] that can be reused for different io_slices
/// with different lifetime.
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
        let mut v = ManuallyDrop::new(Vec::<MaybeUninit<IoSlice<'_>>>::with_capacity(cap.get()));

        debug_assert_eq!(v.capacity(), cap.get());
        debug_assert_eq!(v.len(), 0);

        let ptr = v.as_mut_ptr();

        // Safety:
        //
        //  - ptr is allocated using Vec::with_capacity, with non-zero cap
        //  - Vec::as_mut_ptr returns a non-null, valid pointer upon
        //    a valid allocation.
        //  - It is valid after the vec is dropped since it is wrapped
        //    in ManuallyDrop.
        let ptr = unsafe { NonNull::new_unchecked(ptr as *mut ()) };

        Self { ptr, cap }
    }

    /// Return the underlying io_slices
    pub fn get(&mut self) -> &mut [MaybeUninit<IoSlice<'_>>] {
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
