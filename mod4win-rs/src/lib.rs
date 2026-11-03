#![allow(
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::field_reassign_with_default
)]

#[cfg(feature = "diff-suite")]
pub mod diff_suite;
pub mod engine;
pub mod ui;
#[cfg(feature = "retro-win32")]
pub mod win32s_shim;
