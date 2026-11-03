#![allow(
    clippy::chunks_exact_to_as_chunks,
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::derivable_impls,
    clippy::doc_lazy_continuation,
    clippy::field_reassign_with_default,
    clippy::large_const_arrays,
    clippy::manual_checked_ops,
    clippy::manual_div_ceil,
    clippy::manual_is_multiple_of,
    clippy::manual_range_contains,
    clippy::manual_range_patterns,
    clippy::needless_late_init,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::unnecessary_cast
)]

pub mod formats;
pub use formats as loader;
pub mod it_engine;
pub mod mixer;
pub mod sequencer;
pub mod tables;
pub mod types;



