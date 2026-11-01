//! CSS engine: parser, selectors, cascade, and property resolution.
//!
//! The CSS engine turns stylesheet text into per-node `ComputedStyle` values
//! that the layout engine consumes.

pub mod cascade;
pub mod parser;
pub mod properties;
pub mod selector;

pub use cascade::Cascade;
pub use parser::{parse, Stylesheet, Rule, StyleRule, Declaration, Value, Unit};
pub use selector::Selector;
