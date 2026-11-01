//! Inline layout helpers. Currently the work happens in `block::layout_inline_run`
//! — this module exists for future splitting into separate inline formatting
//! contexts (e.g. for `display: inline-block`).

pub use super::block::layout_inline_run;
