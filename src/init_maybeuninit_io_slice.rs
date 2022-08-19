use std::{
    io::IoSlice,
    iter::{IntoIterator, Iterator},
    mem::MaybeUninit,
};

pub fn init_maybeuninit_io_slices_mut<'a, 'io_slice, Iterable>(
    uninit_io_slices: &'a mut [MaybeUninit<IoSlice<'io_slice>>],
    iterable: Iterable,
) -> &'a mut [IoSlice<'io_slice>]
where
    Iterable: IntoIterator<Item = IoSlice<'io_slice>>,
{
    let mut cnt = 0;

    uninit_io_slices
        .iter_mut()
        .zip(iterable)
        .for_each(|(uninit_io_slice, io_slice)| {
            uninit_io_slice.write(io_slice);
            cnt += 1;
        });

    // Safety:
    //
    //  - uninit_io_slices[..cnt] is initialized using iterable.
    //  - MaybeUninit is a transparent type
    unsafe { &mut *((&mut uninit_io_slices[..cnt]) as *mut _ as *mut [IoSlice<'io_slice>]) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IoSliceExt;

    #[test]
    fn test() {
        let mut uninit_io_slices = [MaybeUninit::<IoSlice<'_>>::uninit(); 5];
        let io_slices = [
            IoSlice::new(b"1023x"),
            IoSlice::new(b"1qwe"),
            IoSlice::new(b"''weqdq"),
            IoSlice::new(b"jiasodjx"),
            IoSlice::new(b"aqw34f"),
        ];
        assert_io_slices_eq(
            init_maybeuninit_io_slices_mut(&mut uninit_io_slices, io_slices),
            &io_slices,
        );
    }

    fn assert_io_slices_eq(x: &[IoSlice<'_>], y: &[IoSlice<'_>]) {
        assert_eq!(x.len(), y.len());

        let x: Vec<&[u8]> = x.iter().copied().map(IoSliceExt::into_inner).collect();
        let y: Vec<&[u8]> = y.iter().copied().map(IoSliceExt::into_inner).collect();

        assert_eq!(x, y);
    }
}
