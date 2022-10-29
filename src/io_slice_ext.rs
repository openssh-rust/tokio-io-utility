use std::{
    io::{IoSlice, IoSliceMut},
    slice::{from_raw_parts, from_raw_parts_mut},
};

pub trait IoSliceExt<'a> {
    fn into_inner(self) -> &'a [u8];
}

impl<'a> IoSliceExt<'a> for IoSlice<'a> {
    fn into_inner(self) -> &'a [u8] {
        let slice = &*self;
        unsafe { from_raw_parts(slice.as_ptr(), slice.len()) }
    }
}

pub trait IoSliceMutExt<'a> {
    fn into_inner(self) -> &'a mut [u8];
}

impl<'a> IoSliceMutExt<'a> for IoSliceMut<'a> {
    fn into_inner(mut self) -> &'a mut [u8] {
        let slice = &mut *self;
        unsafe { from_raw_parts_mut(slice.as_mut_ptr(), slice.len()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_slice_ext() {
        let buffer = b"123455";
        let inner: &'static [u8] = IoSlice::new(buffer).into_inner();

        assert_eq!(inner, buffer);
    }

    #[test]
    fn test_io_slice_mut_ext() {
        let mut buffer = b"123455".to_vec();
        let inner: &mut [u8] = IoSliceMut::new(&mut *buffer).into_inner();

        assert_eq!(inner, b"123455");
    }
}
