//! JavaScript engine integration.
//!
//! We embed [Boa](https://github.com/boa-dev/boa), a pure-Rust ECMAScript
//! implementation, and bridge it to the DOM. Boa is **not** a browser engine —
//! it is a standalone JS runtime — so this satisfies the "no existing browser
//! engine" rule.
//!
//! The bridge works as follows:
//!  * The `JsRuntime` owns a Boa `Context` and a map from node-id → `NodeRef`.
//!  * When JS calls `document.getElementById('foo')`, we look up the node and
//!    return a JS object whose internal data is the node id.
//!  * When JS calls `el.addEventListener('click', fn)`, we store `fn` in the
//!    Boa context (as a `JsFunction`) and register a numeric handler id on
//!    the DOM node. When the event fires, the browser thread looks up the
//!    function by id and calls it.
//!  * We provide a small `console`, `setTimeout`, `setInterval`, `fetch`,
//!    `localStorage` (in-memory), and a basic `window` object.

pub mod dom_api;
pub mod globals;
pub mod runtime;

pub use runtime::{JsRuntime, JsValue, JsError, HandlerId};
