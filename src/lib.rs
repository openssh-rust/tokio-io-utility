// only enables the nightly `doc_cfg` feature when
// the `docsrs` configuration attribute is defined
#![cfg_attr(docsrs, feature(doc_cfg))]

mod async_read_utility;
mod async_write_utility;

#[cfg(feature = "mpsc")]
#[cfg_attr(docsrs, doc(cfg(feature = "mpsc")))]
pub mod queue;

pub use async_read_utility::read_exact_to_vec;
pub use async_write_utility::write_vectored_all;
