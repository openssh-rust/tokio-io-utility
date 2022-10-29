// only enables the nightly `doc_cfg` feature when
// the `docsrs` configuration attribute is defined
#![cfg_attr(docsrs, feature(doc_cfg))]

/// Replacement for [`std::task::ready`].
#[macro_export]
macro_rules! ready {
    ($e:expr) => {
        match $e {
            Poll::Ready(t) => t,
            Poll::Pending => return Poll::Pending,
        }
    };
}

pub fn assert_send<T>(val: T) -> T
where
    T: Send,
{
    val
}

mod async_read_utility;
pub use async_read_utility::*;

mod async_write_utility;
pub use async_write_utility::write_vectored_all;

mod init_maybeuninit_io_slice;
pub use init_maybeuninit_io_slice::init_maybeuninit_io_slices_mut;

mod io_slice_ext;
pub use io_slice_ext::{IoSliceExt, IoSliceMutExt};

mod reusable_io_slices;
pub use reusable_io_slices::ReusableIoSlices;
