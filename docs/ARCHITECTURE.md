# Architecture

This document describes the internal architecture of Z Browser.

## Overview

Z Browser is a Cargo workspace with four crates that mirror the conceptual layers of a browser:

```
┌──────────────────────────────────────────┐
│             browser-app (bin)            │  ← main()
├──────────────────────────────────────────┤
│             browser-ui                   │  ← Window, tabs, chrome
├──────────────────────────────────────────┤
│             browser-render               │  ← Canvas, text, images
├──────────────────────────────────────────┤
│             browser-core                 │  ← HTML, CSS, layout, JS, net
└──────────────────────────────────────────┘
```

`browser-core` has no UI dependencies and can be tested headlessly. `browser-render` paints a DOM into a pixel buffer. `browser-ui` owns the window, pumps events, and drives the page. `browser-app` is a thin main() that initialises logging and starts the UI.

## Data flow on navigation

```
User types URL in address bar
       │
       ▼
Browser::navigate_active(url)
       │
       ▼
Page::navigate(url)
       │
       ├─→ HttpClient::fetch(url)
       │       │
       │       ├─→ Follow redirects (≤10)
       │       ├─→ Check cache (max-age, ETag)
       │       └─→ Return HttpResponse
       │
       ├─→ html::parser::parse(body)        → NodeRef (DOM)
       │
       ├─→ extract_stylesheets(dom)         → Cascade (with linked CSS fetched)
       │
       ├─→ JsRuntime::new(origin, url)
       │       ├─→ install_globals(ctx)
       │       └─→ install_dom(rt)          → document, Element, etc.
       │
       ├─→ Cascade::apply(dom)              → per-node ComputedStyle
       │
       └─→ run_scripts(dom)                 → execute <script> elements in order
               │
               └─→ Fire DOMContentLoaded
```

Then on each frame:

```
Browser::run()
   │
   ├─→ Page::pump_js()                      → fire due timers, drain microtasks
   │
   ├─→ Renderer::render(document, canvas)   → paint backgrounds/borders/text
   │
   ├─→ chrome::render(browser, canvas)      → paint address bar, tabs, buttons
   │
   ├─→ window.update_with_buffer(pixels)
   │
   └─→ handle_events()                      → mouse clicks, keyboard
           │
           ├─→ Click on link → navigate_active(href)
           ├─→ Click on button → page.dispatch_event(node, "click")
           └─→ Address bar Enter → navigate_active(text)
```

## The DOM

`Node` is an `Arc`-counted struct containing a `RwLock<NodeInner>`. Children are stored as strong `Arc`s; the parent link is a `Weak`. Methods like `append_child`, `remove_child`, `insert_before` take `&NodeRef` (i.e. `&Arc<Node>`) so they can install the parent weak reference.

Each node has a globally-unique integer `id`. The JS runtime stores a map `id → NodeRef` so that JS code (which only knows ids) can resolve back to the real `Arc<Node>`.

## The CSS cascade

```rust
struct Cascade { sheets: Vec<Stylesheet> }
```

`Cascade::apply(document, vw, vh)` walks the DOM. For each node it:

1. Collects matching declarations from the UA stylesheet, then author stylesheets, then inline `style=` attribute (which has the highest specificity).
2. Sorts matches by (origin, !important, specificity, source order).
3. Applies each declaration to a `ComputedStyle` struct (starting from a clone of the parent's style for inherited properties).
4. Resolves length units (`em`, `%`, `vw`, …) against the parent's font size and the viewport.
5. Stores the final `ComputedStyle` on the node via `node.set_style(style)`.
6. Recurses into children.

Selectors support descendant/child/adjacent-sibling/general-sibling combinators, all CSS2 attribute operators, a useful subset of pseudo-classes, and `:not(…)`. Specificity is computed as `(id_count, class_count, type_count)`.

## Layout

`layout::layout_document(doc, vw, vh)` walks the DOM and produces a `LayoutBox` tree. Each `LayoutBox` has:

- `rect: LayoutRect` — the border box (x, y, width, height)
- `content: LayoutRect` — the content box (inside padding)
- `children: Vec<LayoutBox>` — block-level children
- `text_lines: Vec<TextLine>` — for inline content

`layout_block(ctx, node, parent_box)` is the workhorse:

1. Compute margin/border/padding/width from the node's `ComputedStyle`.
2. Lay out child block elements recursively, accumulating height.
3. For runs of inline content, call `layout_inline_run`, which wraps text into lines.
4. For `display: flex`, delegate to `layout_flex`. For `display: grid`, delegate to `layout_grid`.
5. After children are laid out, the block's height is the explicit height or the sum of children, and `node.set_layout(rect)` is called so the renderer and JS `getBoundingClientRect` can read it.

Flexbox implements:
- `flex-direction`: row, row-reverse, column, column-reverse
- `justify-content`: flex-start, flex-end, center, space-between, space-around, space-evenly
- `align-items`: stretch, flex-start, flex-end, center, baseline
- `flex-wrap`: nowrap, wrap, wrap-reverse
- `flex-grow` / `flex-shrink` / `flex-basis` distribution

Grid implements:
- `grid-template-columns` / `grid-template-rows` with px, %, fr, auto
- Line-based placement via `grid-column` / `grid-row`
- Auto-placement of items without explicit positions

## JavaScript

The `JsRuntime` owns a `Mutex<Context>` (Boa's execution context). Because Boa is not `Sync`, all JS runs on the thread that owns the runtime — typically the UI thread.

The bridge pattern:

1. Native helpers (`__node_getAttribute(id, name)`, `__node_appendChild(parent_id, child_id)`, …) are registered as global JS functions.
2. A JS shim defines `__makeNode(id)`, which constructs a plain JS object with properties (via `Object.defineProperty`) and methods that call the `__node_*` helpers.
3. `document` is a singleton JS object with methods that call the helpers with id 0 (which the runtime maps to the document node via `set_document`).
4. Event handlers are stored as Boa global properties `__h_<id>`. `addEventListener` allocates an id, stores the function as `__h_<id>`, and registers the id on the DOM node.
5. When an event fires (e.g. user clicks a button), the UI calls `page.dispatch_event(node, "click")`, which iterates the node's listeners and evaluates `__h_<id>()` for each.

A thread-local pointer (`CURRENT_RT`) lets native functions retrieve the runtime without it being passed through Boa's call signature. `evaluate()` sets this thread-local before calling `ctx.eval` and clears it after.

`setTimeout`/`setInterval` queue `Timer` structs on the runtime. `pump()` (called from the UI loop) fires due timers and drains the microtask queue.

`fetch` performs a synchronous HTTP request via `HttpClient::fetch` and returns a resolved `Promise<Response>`. The `Response` object has `text()` and `json()` methods that return Promises (resolved synchronously, since the body is already in memory).

## Networking

`HttpClient` wraps a `ureq::Agent` configured with:
- 30-second timeout
- Manual redirect handling (capped at 10)
- Mozilla root CA bundle via `webpki-roots`

On top of ureq we add:
- **Cookie jar**: in-memory `HashMap<host, Vec<StorableCookie>>`. Cookies are sent on requests and parsed from `Set-Cookie` on responses. `Secure` and `Path` attributes are honoured.
- **Cache**: in-memory `HashMap<url, CachedResponse>`. `max-age` from `Cache-Control` controls expiry; `ETag` is sent as `If-None-Match` for revalidation.
- **Schemes**: `http`, `https`, `data` (with base64 support), `file` (reads from disk).

`HttpResponse` exposes `content_type()`, `is_html()`, `is_css()`, `is_js()`, `is_image()` helpers used by the page loader.

## Rendering

`Renderer` owns:
- Three `Arc<FontArc>` instances: serif, sans-serif, monospace (loaded from `/usr/share/fonts/truetype/dejavu/` or Liberation fallbacks)
- An `ImageCache` (in-memory `HashMap<url, DecodedImage>`)

`Renderer::render(document, canvas)` walks the DOM and, for each visible element:
1. Fills the background color (alpha-blended)
2. Draws the four border edges (only solid 1px-style for now)
3. Rasterizes text via `ab_glyph` and alpha-blends each pixel

The `Canvas` is a `Vec<u32>` of `0xRRGGBBAA` pixels, blitted directly to `minifb::Window::update_with_buffer`.

## Browser UI

`Browser` owns:
- A `minifb::Window`
- A `Vec<Tab>` (each tab is an `Arc<Page>` plus a per-tab `History`)
- A global `History` (for the history view)
- A `Vec<Bookmark>` (hardcoded defaults for now)
- An `address_input: Mutex<String>` and `address_focused: Mutex<bool>` for the address bar

The event loop runs at ~60fps. Each frame:
1. `page.pump_js()` fires due timers
2. The page is rendered into a `Canvas`
3. `chrome::render` paints the tab bar, address bar, and buttons on top
4. `window.update_with_buffer` blits the canvas
5. `handle_events` processes mouse and keyboard input

`chrome::handle_click` does hit-testing for the chrome (tabs, address bar, buttons). For page content, `find_element_at` walks the DOM and returns the topmost element whose `LayoutRect` contains the click point. Clicks on `<a>` trigger `navigate_active`; clicks on form submit buttons trigger `submit_form`, which gathers input values and navigates to the action URL.

## Security

- **Same-Origin Policy**: `security::is_same_origin(a, b)` compares scheme+host+port. Used by the DOM access check (currently always true for the page's own scripts; cross-origin access is blocked by not loading cross-origin scripts at all unless `<script src=...>` is used, in which case the script runs in the page's origin).
- **TLS**: ureq uses `rustls` with `webpki-roots` for certificate verification. Invalid certs cause the request to fail.
- **JS isolation**: each `Page` has its own `JsRuntime`; tabs do not share JS state.
- **CORS**: `is_cors_exempt` allows same-origin, `data:`, `file:`, and `blob:` URLs; everything else would require CORS checks (we don't currently send preflight `OPTIONS` requests — known limitation).
- **Cookies**: `Secure` cookies are only sent over HTTPS; `HttpOnly` is parsed but not enforced on JS access (known limitation).

## Testing

Tests live in `crates/browser-core/tests/` and `crates/browser-app/tests/`:

- `html_parser.rs` — 7 tests covering tags, attributes, void elements, entities, auto-closing, comments, raw text
- `css_parser.rs` — 15 tests covering rules, selectors, specificity, colors, !important, media queries
- `js_engine.rs` — 13 tests covering arithmetic, closures, classes, promises, arrays, DOM API
- `layout.rs` — 3 tests covering block, flex, and basic document layout
- `networking.rs` — 6 tests covering URL parsing, data URLs, base64, cookies, same-origin
- `navigation.rs` — 4 tests covering page navigation, inline scripts, DOMContentLoaded, multiple pages

The test HTML pages under `tests/pages/` can be loaded in the running browser to manually verify features.

## Why Boa is allowed

The challenge says: *"You may implement a JavaScript engine yourself or integrate a standalone JavaScript runtime, provided it is not part of an existing browser engine."*

Boa is a standalone ECMAScript engine. It is not part of Chromium, WebKit, Gecko, or any other browser engine. It implements the ECMAScript Language Specification and the ECMAScript Internationalization API — nothing DOM-specific. The DOM API bridge (`document`, `Element`, `setTimeout`, `fetch`, …) is implemented in this repository, in `crates/browser-core/src/js/`.
