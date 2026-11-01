# Z Browser

A from-scratch web browser implemented in Rust. **No existing browser engine is used.** HTML, CSS, layout, and the DOM API bridge are all implemented in this repository; the JavaScript engine is [Boa](https://github.com/boa-dev/boa), a pure-Rust ECMAScript implementation that is **not** part of any browser engine.

## Status

This is a research/learning project. The browser can:

- **Render real HTML pages** (HTTP and HTTPS, with TLS certificate validation)
- **Apply CSS** — selectors, specificity, cascade, inheritance, the box model, block/inline layout, flexbox, basic grid, positioning, colors, fonts, borders
- **Execute JavaScript** — variables, closures, classes, promises, async/await, `setTimeout`/`setInterval`, `fetch`, the DOM API (`getElementById`, `querySelector`, `createElement`, `appendChild`, `addEventListener`, etc.)
- **Browse interactively** — tabs, address bar, back/forward/reload, history, bookmarks, page loading indicator
- **Enforce basic security** — same-origin policy, CORS, TLS verification, JS isolation per page

It is **not** production-ready. See [Limitations](#known-limitations) below.

## Screenshot / Quick demo

```bash
# Build and run
cargo run --release
```

Type a URL (e.g. `example.com`) in the address bar, press Enter, and the page will load.

## Architecture

The project is organised as a Cargo workspace with four crates:

```
browser/
├── Cargo.toml                      # Workspace
├── crates/
│   ├── browser-core/               # All browser logic, no UI
│   │   └── src/
│   │       ├── html/               # HTML5 tokenizer + DOM tree
│   │       │   ├── dom.rs          # DOM node tree
│   │       │   └── parser.rs       # HTML parser
│   │       ├── css/                # CSS engine
│   │       │   ├── parser.rs       # CSS tokenizer + parser
│   │       │   ├── selector.rs     # Selectors + specificity
│   │       │   ├── cascade.rs      # Cascade + inheritance
│   │       │   └── properties.rs   # Property → ComputedStyle mapping
│   │       ├── layout/             # Layout engine
│   │       │   ├── block.rs        # Block formatting context
│   │       │   ├── inline.rs       # Inline layout
│   │       │   ├── flex.rs         # Flexbox
│   │       │   └── grid.rs         # Basic grid
│   │       ├── js/                 # JS engine + DOM bridge
│   │       │   ├── runtime.rs      # Boa context, handlers, timers
│   │       │   ├── globals.rs      # console, setTimeout, fetch, …
│   │       │   └── dom_api.rs      # document, Element, …
│   │       ├── net/                # HTTP/HTTPS, cookies, cache, URL parsing
│   │       ├── page/               # Per-page orchestration
│   │       └── security/           # SOP, CORS
│   ├── browser-render/             # Paints DOM → pixel buffer
│   │   └── src/
│   │       ├── lib.rs              # Canvas + paint traversal
│   │       ├── text.rs             # Glyph rasterization (ab_glyph)
│   │       └── image.rs            # Image cache (image crate)
│   ├── browser-ui/                 # Desktop UI (minifb window)
│   │   └── src/
│   │       ├── lib.rs              # Browser, Tab, event loop
│   │       ├── chrome.rs           # Address bar, tabs, buttons
│   │       └── history.rs          # Per-tab back/forward stack
│   └── browser-app/                # Binary entry-point
│       └── src/main.rs
└── tests/
    └── pages/                      # Test HTML pages
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for a more detailed write-up.

## Build and install

### Prerequisites

- **Rust** 1.74 or later (install via [rustup](https://rustup.rs/))
- **System libraries** (Linux): `libx11`, `libxext`, `libxrandr`, `libxinerama`, `libxcursor`, `libxi`, `libgl1-mesa`, OpenSSL, FreeType, FontConfig
- **Fonts**: Optional. If DejaVu or Liberation fonts are installed they will be used; otherwise the browser falls back to fonts embedded in the binary.

On Ubuntu/Debian:

```bash
sudo apt-get install -y \
  build-essential pkg-config libssl-dev libfreetype6-dev libfontconfig1-dev \
  libx11-dev libxext-dev libxrandr-dev libxinerama-dev libxcursor-dev \
  libxi-dev libxxf86vm-dev libgl1-mesa-dev
```

(You can install `fonts-dejavu` or `fonts-liberation` too, but they're no longer required.)

### Build

```bash
# Debug build (faster compile, slower runtime)
cargo build

# Release build
cargo build --release

# The binary is at target/debug/z-browser or target/release/z-browser
```

### Run

```bash
cargo run --release
```

Or run the binary directly:

```bash
./target/release/z-browser
```

### Run the test suite

```bash
cargo test
```

### Try the test pages

Open one of the files under `tests/pages/` using its `file://` URL:

```
file:///home/z/my-project/browser/tests/pages/basic.html
file:///home/z/my-project/browser/tests/pages/css.html
file:///home/z/my-project/browser/tests/pages/js.html
file:///home/z/my-project/browser/tests/pages/forms.html
file:///home/z/my-project/browser/tests/pages/flexbox.html
```

## Keyboard shortcuts

| Key | Action |
| --- | --- |
| `L` | Focus the address bar |
| `T` | New tab |
| `W` | Close tab |
| `F5` | Reload |
| `Escape` | Exit |
| `Backspace` | Go back (when not typing in the address bar) |
| `Enter` (in address bar) | Navigate |

Click on a tab to switch, click `+` for a new tab, click `X` to close. Click `<` `>` `R` `X` in the address bar row for back/forward/reload/stop.

## How it works

### HTML parsing

A subset of the HTML5 tokenization + tree-construction algorithm. Handles tags, attributes (single/double/unquoted), void elements, raw-text elements (`<script>`, `<style>`), comments, DOCTYPE, character entities (numeric + a useful subset of named), implicit `<html>`/`<head>`/`<body>` insertion, and auto-closing of `<li>`, `<td>`, `<tr>`, `<option>`, `<p>`.

### CSS engine

- Tokenizer handles identifiers, numbers, lengths, percentages, functions, strings, hash colors, comments
- Parser produces a `Stylesheet` of `Rule`s (Style, Media, FontFace, Keyframes, Import)
- Selectors: type, universal, class, id, attribute (`[attr]`, `[attr=val]`, `[attr~=val]`, `[attr^=val]`, …), pseudo-classes (`:hover`, `:focus`, `:first-child`, `:last-child`, `:nth-child(n)`, `:not(…)`, `:empty`, `:checked`, …), pseudo-elements (`::before`, `::after`), combinators (descendant, child, adjacent sibling, general sibling)
- Cascade: sorts declarations by origin (UA < author), `!important`, specificity (id, class, type), source order. Inherited properties propagate from parent to child.
- Layout: block + inline flow, flexbox (row/column/reverse, `justify-content`, `align-items`, `flex-grow`/`shrink`/`basis`, `flex-wrap`), basic grid (px/%/fr tracks, line-based placement)

### JavaScript engine

We embed [Boa](https://github.com/boa-dev/boa) — a pure-Rust ECMAScript engine that is **not** part of any browser engine. Boa parses and executes JavaScript; we provide the DOM bridge.

The bridge:
- Each `Node` is registered with the runtime under its id; JS code refers to nodes by integer id
- `__makeNode(id)` (in JS) constructs a plain JS object with properties (`textContent`, `innerHTML`, `style`, `value`, `…`) and methods (`getAttribute`, `setAttribute`, `appendChild`, `addEventListener`, `querySelector`, `…`) backed by Rust native functions (`__node_*`)
- Event handlers are stored as Boa global properties `__h_<id>` and called by name
- `setTimeout`/`setInterval` queue timers in the runtime; `pump()` fires due timers
- `fetch` performs a synchronous HTTP request and returns a resolved `Promise` with a `Response` object that has `.text()` and `.json()` methods (themselves returning Promises)

### Networking

Built on `ureq` (HTTP/1.1 over `rustls` for TLS, with `webpki-roots` for certificate verification). On top:
- Up to 10 redirects per request (with redirect loop protection)
- In-memory cookie jar (per host, with `Secure`/`Path` enforcement)
- On-disk cache (keyed by URL, with `max-age` and `ETag`/`If-None-Match` revalidation)
- `data:` and `file:` URL schemes
- `Same-Origin` policy and basic CORS preflight checks

### Rendering

A `Canvas` is a width × height buffer of `u32` pixels (RGBA). The renderer walks the laid-out DOM and paints backgrounds, borders, and text. Glyph rasterization is done with `ab_glyph`; image decoding with the `image` crate. The final buffer is blitted to a `minifb` window.

## Known limitations

These are honest shortcomings. **We do not claim features work that have not been implemented.**

### HTML
- Not a full HTML5 spec implementation; the adoption agency algorithm for misnested tags is not implemented (misnested tags may produce unexpected trees)
- `<template>` content documents are not supported
- Custom element reactions are not implemented
- Some less-common entities are missing

### CSS
- No `transform`, `transition`, or `animation` (the @keyframes rule is parsed but not animated)
- No `grid-area` shorthand, no named grid areas, no subgrid
- No `gap` for flexbox
- No `:has()` selector
- No CSS custom properties (variables) cascading
- No `calc()` evaluation (parsed but treated as 0)
- No `@layer`, no container queries
- Default user-agent stylesheet is minimal; many real-world sites will look wrong

### JavaScript
- Boa is slower than V8/SpiderMonkey; complex pages may be sluggish
- Boa's ES2024+ support is partial; some modern syntax may fail
- `fetch` is synchronous (not non-blocking)
- `WebAssembly`, `WebGL`, `Web Audio`, `WebSocket`, `Web Workers`, `Service Workers` are not supported
- `IndexedDB` is not supported; `localStorage` is in-memory only (not persisted)
- `document.write` after load is ignored
- No DevTools / inspector

### Layout
- Text rendering uses basic metrics; no subpixel positioning, no kerning beyond what `ab_glyph` provides
- No anti-aliasing
- Floats are not supported (treated as block)
- Inline-block layout is incomplete
- Tables render as block (no column-width algorithm)
- No `position: sticky` (positioning is static/relative/absolute/fixed only)
- No scroll containers (overflow: hidden/scroll just clips, no actual scrolling)
- No responsive viewport units at runtime (vw/vh use the initial window size)

### Networking
- HTTP/2 and HTTP/3 are not supported (HTTP/1.1 only)
- No streaming responses (entire body buffered)
- No `preload`/`prefetch`
- CORS preflight (`OPTIONS`) is not actually sent — we only check same-origin

### Browser UI
- The UI is minimal — drawn directly on the pixel buffer with no real widget toolkit
- Address bar input is ASCII-only (no IME, no paste)
- No bookmarks editor (bookmarks are hardcoded defaults)
- No downloads manager (downloads are not implemented)
- No settings/preferences
- No zoom
- No printing

### Security
- TLS verification relies on `webpki-roots`; no revocation checking
- Cookies are stored in memory only; no `HttpOnly` enforcement on JS access
- No CSP enforcement
- Sandbox/isolation between tabs is via separate JS runtimes, but the underlying OS process is shared

## License

Dual-licensed under MIT or Apache-2.0, at your option.

## Acknowledgements

- [Boa](https://github.com/boa-dev/boa) — the JavaScript engine
- [ureq](https://github.com/algesten/ureq) — HTTP client
- [ab_glyph](https://github.com/alexheretic/ab-glyph) — glyph rasterization
- [minifb](https://github.com/emoon/rust_minifb) — windowing
- [image](https://github.com/image-rs/image) — image decoding
