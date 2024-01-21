#!/bin/sh

export RUSTDOCFLAGS="--cfg tokio_io_utility_docsrs"
exec cargo +nightly doc --all-features
