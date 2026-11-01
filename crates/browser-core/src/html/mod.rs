//! HTML parsing and the Document Object Model.
//!
//! The DOM is the central data structure of the browser. Every other subsystem
//! reads from it or writes to it.

pub mod dom;
pub mod parser;

pub use dom::*;
pub use parser::parse;
