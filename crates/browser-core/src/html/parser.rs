//! HTML5 tokenizer + tree-builder.
//!
//! This is a deliberately simplified implementation of the HTML5 parsing
//! algorithm. It is **not** a spec-compliant implementation of the full state
//! machine — that is enormous and outside the scope of this project. Instead
//! we implement the subset that handles real-world web pages "well enough":
//!
//!  * Tags with attributes (single, double, and unquoted values)
//!  * Self-closing tags (`<br/>`)
//!  * Void elements (`<img>`, `<input>`, …)
//!  * Raw-text elements (`<script>`, `<style>`, `<textarea>`, `<title>`)
//!  * Comments and DOCTYPE
//!  * Named character references (a useful subset)
//!  * Implicit `<html>`, `<head>`, `<body>` insertion
//!  * Auto-closing of `<li>`, `<td>`, `<tr>`, `<option>`, `<p>` etc.
//!
//! What we explicitly do **not** do:
//!  * The full adoption agency algorithm (misnested tags are mostly preserved
//!    in source order rather than reconstructed per spec).
//!  * `<template>` content documents.
//!  * Custom element reactions.
//!  * Scripting-enabled vs scripting-disabled parsing differences.

use super::dom::{self, *};
use smol_str::SmolStr;
use std::collections::HashMap;

/// Parse a string of HTML into a DOM tree rooted at a Document node.
pub fn parse(input: &str) -> NodeRef {
    let mut p = Parser::new(input);
    p.run();
    p.document.clone()
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    /// The document node.
    document: NodeRef,
    /// Open-element stack (most recently opened is at the end).
    open_elements: Vec<NodeRef>,
    /// Insertion mode. We only need a coarse approximation.
    mode: Mode,
    /// Head element, if we've seen one.
    head: Option<NodeRef>,
    /// Whether we've inserted `<body>` yet.
    body_inserted: bool,
    /// Cached entity table.
    entities: HashMap<&'static str, &'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    AfterHead,
    InBody,
    AfterBody,
    AfterAfterBody,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        let document = dom::document();
        Self {
            src: input.as_bytes(),
            pos: 0,
            document: document.clone(),
            open_elements: Vec::new(),
            mode: Mode::Initial,
            head: None,
            body_inserted: false,
            entities: entities_table(),
        }
    }

    fn run(&mut self) {
        let mut safety = 0usize;
        while self.pos < self.src.len() {
            safety += 1;
            if safety > 10_000_000 {
                log::error!("HTML parser: infinite loop detected at pos {}", self.pos);
                break;
            }
            match self.mode {
                Mode::Initial | Mode::BeforeHtml => {
                    self.run_prologue();
                }
                Mode::BeforeHead | Mode::InHead | Mode::AfterHead => {
                    self.run_head_phase();
                }
                Mode::InBody | Mode::AfterBody | Mode::AfterAfterBody => {
                    self.run_body_phase();
                }
            }
        }
    }

    fn run_prologue(&mut self) {
        // Skip leading whitespace, BOM, comments, doctype; then ensure html/head.
        self.skip_whitespace();
        if self.pos >= self.src.len() {
            return;
        }
        if self.starts_with("<!--") {
            self.consume_comment();
            return;
        }
        if self.starts_with_ci("<!doctype") {
            self.consume_doctype();
            return;
        }
        if self.starts_with_ci("<html") {
            let html = self.consume_start_tag();
            Node::append_child(&self.document, html.clone());
            self.open_elements.push(html);
            self.mode = Mode::BeforeHead;
            return;
        }
        // Implicit <html>
        let html = dom::element("html");
        Node::append_child(&self.document, html.clone());
        self.open_elements.push(html);
        self.mode = Mode::BeforeHead;
    }

    fn run_head_phase(&mut self) {
        self.skip_whitespace();
        if self.pos >= self.src.len() {
            return;
        }
        if self.starts_with("<!--") {
            self.consume_comment();
            return;
        }
        if self.starts_with_ci("<head") && self.head.is_none() {
            let head = self.consume_start_tag();
            if let Some(parent) = self.open_elements.last() {
                Node::append_child(parent, head.clone());
            }
            self.open_elements.push(head.clone());
            self.head = Some(head);
            self.mode = Mode::InHead;
            return;
        }
        if self.starts_with_ci("</head") {
            self.consume_end_tag();
            // Pop head
            self.open_elements.pop();
            self.mode = Mode::AfterHead;
            return;
        }
        if self.starts_with_ci("<head") {
            // Duplicate <head> — ignore the tag.
            self.consume_start_tag();
            return;
        }
        // Head-only elements encountered in BeforeHead/InHead/AfterHead.
        if self.is_head_element() {
            if self.head.is_none() {
                // Insert <head> implicitly.
                let head = dom::element("head");
                if let Some(parent) = self.open_elements.last() {
                    Node::append_child(parent, head.clone());
                }
                self.open_elements.push(head.clone());
                self.head = Some(head);
                self.mode = Mode::InHead;
            }
            let tag = self.consume_start_tag();
            if !is_void_element(&tag.tag_name().unwrap_or_default()) {
                if let Some(parent) = self.open_elements.last() {
                    Node::append_child(parent, tag.clone());
                }
                self.open_elements.push(tag);
            } else if let Some(parent) = self.open_elements.last() {
                Node::append_child(parent, tag);
            }
            return;
        }
        if self.starts_with_ci("<body") {
            // Close head if open
            if self.mode == Mode::InHead {
                self.open_elements.pop();
            }
            let body = self.consume_start_tag();
            if let Some(parent) = self.open_elements.last() {
                Node::append_child(parent, body.clone());
            }
            self.open_elements.push(body);
            self.body_inserted = true;
            self.mode = Mode::InBody;
            return;
        }
        // Anything else: close head and switch to body.
        if self.mode == Mode::InHead {
            self.open_elements.pop();
        }
        if !self.body_inserted {
            let body = dom::element("body");
            if let Some(parent) = self.open_elements.last() {
                Node::append_child(parent, body.clone());
            }
            self.open_elements.push(body);
            self.body_inserted = true;
        }
        self.mode = Mode::InBody;
    }

    fn run_body_phase(&mut self) {
        if self.pos >= self.src.len() {
            return;
        }
        if self.starts_with("<!--") {
            self.consume_comment();
            return;
        }
        if self.starts_with_ci("<!doctype") {
            self.consume_doctype();
            return;
        }
        if self.starts_with("</") {
            self.handle_end_tag();
            return;
        }
        if self.peek_byte() == Some(b'<') {
            self.handle_start_tag();
            return;
        }
        // Text — consume up to next '<'
        let start = self.pos;
        while self.pos < self.src.len() && self.src[self.pos] != b'<' {
            self.pos += 1;
        }
        let raw = &self.src[start..self.pos];
        let text = self.decode_entities(raw);
        let text = text.trim_matches(|c: char| c == '\r' || c == '\n' || c == '\t' || c == ' ');
        if !text.is_empty() {
            let node = dom::text(text);
            if let Some(parent) = self.current_insertion_point() {
                Node::append_child(&parent, node);
            }
        }
    }

    /// Decide where to insert the next node. This handles cases where the
    /// top of the open-elements stack is a raw-text element or similar.
    fn current_insertion_point(&self) -> Option<NodeRef> {
        self.open_elements.last().cloned()
    }

    fn is_head_element(&self) -> bool {
        // Peek tag name
        if !self.starts_with("<") {
            return false;
        }
        let mut i = self.pos + 1;
        while i < self.src.len() && self.src[i].is_ascii_alphabetic() {
            i += 1;
        }
        let name = std::str::from_utf8(&self.src[self.pos + 1..i])
            .unwrap_or("")
            .to_ascii_lowercase();
        matches!(
            name.as_str(),
            "title" | "base" | "link" | "meta" | "style" | "script" | "noscript"
        )
    }

    fn handle_start_tag(&mut self) {
        // First, peek the tag name to decide if it needs special handling.
        let name = self.peek_tag_name();
        let name_lower = name.to_ascii_lowercase();
        // Auto-close rules
        if let Some(top) = self.open_elements.last() {
            if let Some(top_tag) = top.tag_name() {
                if dom::auto_closes(&top_tag, &name_lower) {
                    self.open_elements.pop();
                }
            }
        }
        // Implicit <body>
        if !self.body_inserted {
            let body = dom::element("body");
            if let Some(parent) = self.open_elements.last() {
                Node::append_child(parent, body.clone());
            }
            self.open_elements.push(body);
            self.body_inserted = true;
        }

        let is_void = dom::is_void_element(&name_lower);
        let is_raw = dom::is_raw_text_element(&name_lower);

        let node = self.consume_start_tag();
        if let Some(parent) = self.open_elements.last() {
            Node::append_child(parent, node.clone());
        }

        if is_raw {
            // Consume raw text until matching close tag.
            let raw = self.consume_raw_text(&name_lower);
            if !raw.is_empty() {
                let text_node = dom::text(&raw);
                Node::append_child(&node, text_node);
            }
            // Expect a close tag now
            if self.starts_with_ci(&format!("</{}", name_lower)) {
                self.consume_end_tag();
            }
            return;
        }
        if !is_void {
            self.open_elements.push(node);
        }
    }

    fn handle_end_tag(&mut self) {
        let name = self.peek_end_tag_name().to_ascii_lowercase();
        self.consume_end_tag();
        // Find the nearest matching open element.
        let mut idx = None;
        for (i, n) in self.open_elements.iter().enumerate().rev() {
            if n.tag_name().map(|t| t.to_lowercase()) == Some(name.clone()) {
                idx = Some(i);
                break;
            }
        }
        if let Some(i) = idx {
            self.open_elements.truncate(i);
        }
        // If no match, ignore the stray end tag (HTML5 behaviour).
    }

    // ----- low-level tokenizer helpers -----

    fn peek_byte(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn starts_with(&self, s: &str) -> bool {
        self.src[self.pos..].starts_with(s.as_bytes())
    }

    fn starts_with_ci(&self, s: &str) -> bool {
        let s = s.as_bytes();
        if self.src.len() - self.pos < s.len() {
            return false;
        }
        for (i, b) in s.iter().enumerate() {
            if self.src[self.pos + i].to_ascii_lowercase() != *b {
                return false;
            }
        }
        true
    }

    fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek_byte() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek_tag_name(&self) -> String {
        // Assumes self.pos points at '<'.
        let mut i = self.pos + 1;
        // skip '!' or '/' if present
        if i < self.src.len() && (self.src[i] == b'!' || self.src[i] == b'/') {
            i += 1;
        }
        let start = i;
        while i < self.src.len() && (self.src[i].is_ascii_alphanumeric() || self.src[i] == b'-') {
            i += 1;
        }
        String::from_utf8_lossy(&self.src[start..i]).into_owned()
    }

    fn peek_end_tag_name(&self) -> String {
        // Assumes self.src[pos..] starts with "</"
        let mut i = self.pos + 2;
        let start = i;
        while i < self.src.len() && self.src[i] != b'>' && self.src[i] != b' ' {
            i += 1;
        }
        String::from_utf8_lossy(&self.src[start..i]).into_owned()
    }

    fn consume_start_tag(&mut self) -> NodeRef {
        // Skip '<'
        self.pos += 1;
        let name_start = self.pos;
        while self.pos < self.src.len()
            && (self.src[self.pos].is_ascii_alphanumeric() || self.src[self.pos] == b'-')
        {
            self.pos += 1;
        }
        let name = String::from_utf8_lossy(&self.src[name_start..self.pos]).into_owned();
        let node = dom::element(&name.to_ascii_lowercase());

        // Attributes
        loop {
            self.skip_in_tag_whitespace();
            if let Some(b) = self.peek_byte() {
                if b == b'>' {
                    self.pos += 1;
                    break;
                }
                if b == b'/' {
                    self.pos += 1;
                    // Self-closing — skip to '>'
                    while let Some(b) = self.peek_byte() {
                        self.pos += 1;
                        if b == b'>' {
                            break;
                        }
                    }
                    break;
                }
            } else {
                break;
            }
            // Attribute name
            let an_start = self.pos;
            while let Some(b) = self.peek_byte() {
                if b == b'=' || b == b'>' || b == b'/' || b == b' ' || b == b'\t' || b == b'\n'
                    || b == b'\r'
                {
                    break;
                }
                self.pos += 1;
            }
            if self.pos == an_start {
                // Stuck — advance to avoid infinite loop
                self.pos += 1;
                continue;
            }
            let attr_name = String::from_utf8_lossy(&self.src[an_start..self.pos])
                .into_owned()
                .to_ascii_lowercase();
            self.skip_in_tag_whitespace();
            // Optional '=' value
            let mut value = String::new();
            if self.peek_byte() == Some(b'=') {
                self.pos += 1;
                self.skip_in_tag_whitespace();
                let quote = self.peek_byte();
                match quote {
                    Some(b'"') | Some(b'\'') => {
                        let q = self.peek_byte().unwrap();
                        self.pos += 1;
                        let v_start = self.pos;
                        while self.pos < self.src.len() && self.src[self.pos] != q {
                            self.pos += 1;
                        }
                        value = self.decode_entities(&self.src[v_start..self.pos]);
                        if self.pos < self.src.len() {
                            self.pos += 1; // closing quote
                        }
                    }
                    _ => {
                        let v_start = self.pos;
                        while let Some(b) = self.peek_byte() {
                            if b == b'>' || b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                                break;
                            }
                            self.pos += 1;
                        }
                        value = self.decode_entities(&self.src[v_start..self.pos]);
                    }
                }
            }
            {
                node.set_attribute(&attr_name, &value);
            }
        }

        node
    }

    fn consume_end_tag(&mut self) {
        // Skip "</name>"
        self.pos += 2; // </
        while let Some(b) = self.peek_byte() {
            self.pos += 1;
            if b == b'>' {
                break;
            }
        }
    }

    fn consume_comment(&mut self) {
        // Skip "<!--"
        self.pos += 4;
        let start = self.pos;
        while self.pos + 2 < self.src.len() {
            if &self.src[self.pos..self.pos + 3] == b"-->" {
                break;
            }
            self.pos += 1;
        }
        let text = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
        let node = Node::new(NodeKind::Comment(SmolStr::new(text)));
        if let Some(parent) = self.current_insertion_point() {
            Node::append_child(&parent, node);
        }
        // Skip "-->"
        if self.pos + 3 <= self.src.len() {
            self.pos += 3;
        }
    }

    fn consume_doctype(&mut self) {
        // Skip "<!doctype"
        let mut i = self.pos;
        while i < self.src.len() && self.src[i] != b'>' {
            i += 1;
        }
        let raw = String::from_utf8_lossy(&self.src[self.pos..i]).into_owned();
        self.pos = i + 1;
        let name = raw
            .split_whitespace()
            .nth(1)
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let node = Node::new(NodeKind::Doctype(DoctypeData {
            name: SmolStr::new(name),
            ..Default::default()
        }));
        Node::append_child(&self.document, node);
    }

    fn consume_raw_text(&mut self, tag: &str) -> String {
        let close = format!("</{}", tag);
        let start = self.pos;
        while self.pos + close.len() <= self.src.len() {
            if self.src[self.pos..self.pos + close.len()].eq_ignore_ascii_case(close.as_bytes())
            {
                break;
            }
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.src[start..self.pos]).into_owned()
    }

    fn skip_in_tag_whitespace(&mut self) {
        while let Some(b) = self.peek_byte() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn decode_entities(&self, raw: &[u8]) -> String {
        let s = String::from_utf8_lossy(raw);
        let mut out = String::with_capacity(s.len());
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'&' {
                // Find ';'
                let end = bytes[i..].iter().position(|&b| b == b';');
                if let Some(rel) = end {
                    let ent = &bytes[i + 1..i + rel];
                    if !ent.is_empty() && ent[0] == b'#' {
                        // Numeric
                        let num_str = std::str::from_utf8(&ent[1..]).unwrap_or("");
                        let code = if let Some(hex) = num_str.strip_prefix('x').or_else(|| num_str.strip_prefix('X')) {
                            u32::from_str_radix(hex, 16).ok()
                        } else {
                            num_str.parse::<u32>().ok()
                        };
                        if let Some(c) = code.and_then(char::from_u32) {
                            out.push(c);
                            i += rel + 1;
                            continue;
                        }
                    } else if let Ok(name) = std::str::from_utf8(ent) {
                        if let Some(val) = self.entities.get(name) {
                            out.push_str(val);
                            i += rel + 1;
                            continue;
                        }
                    }
                }
                // Fallback: literal '&'
                out.push('&');
                i += 1;
            } else {
                // Copy one UTF-8 char
                let ch_len = utf8_len(bytes[i]);
                out.push_str(std::str::from_utf8(&bytes[i..i + ch_len]).unwrap_or("\u{FFFD}"));
                i += ch_len;
            }
        }
        out
    }
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

fn entities_table() -> HashMap<&'static str, &'static str> {
    let entries: &[(&str, &str)] = &[
        ("amp", "&"),
        ("lt", "<"),
        ("gt", ">"),
        ("quot", "\""),
        ("apos", "'"),
        ("nbsp", "\u{00A0}"),
        ("copy", "\u{00A9}"),
        ("reg", "\u{00AE}"),
        ("trade", "\u{2122}"),
        ("hellip", "\u{2026}"),
        ("mdash", "\u{2014}"),
        ("ndash", "\u{2013}"),
        ("lsquo", "\u{2018}"),
        ("rsquo", "\u{2019}"),
        ("ldquo", "\u{201C}"),
        ("rdquo", "\u{201D}"),
        ("laquo", "\u{00AB}"),
        ("raquo", "\u{00BB}"),
        ("deg", "\u{00B0}"),
        ("plusmn", "\u{00B1}"),
        ("times", "\u{00D7}"),
        ("divide", "\u{00F7}"),
        ("frac12", "\u{00BD}"),
        ("frac14", "\u{00BC}"),
        ("frac34", "\u{00BE}"),
        ("sup2", "\u{00B2}"),
        ("sup3", "\u{00B3}"),
        ("alpha", "\u{03B1}"),
        ("beta", "\u{03B2}"),
        ("gamma", "\u{03B3}"),
        ("delta", "\u{03B4}"),
        ("epsilon", "\u{03B5}"),
        ("pi", "\u{03C0}"),
        ("sigma", "\u{03C3}"),
        ("omega", "\u{03C9}"),
        ("Alpha", "\u{0391}"),
        ("Beta", "\u{0392}"),
        ("Gamma", "\u{0393}"),
        ("Delta", "\u{0394}"),
        ("Omega", "\u{03A9}"),
        ("euro", "\u{20AC}"),
        ("pound", "\u{00A3}"),
        ("yen", "\u{00A5}"),
        ("cent", "\u{00A2}"),
        ("sect", "\u{00A7}"),
        ("para", "\u{00B6}"),
        ("middot", "\u{00B7}"),
        ("bull", "\u{2022}"),
        ("dagger", "\u{2020}"),
        ("Dagger", "\u{2021}"),
        ("permil", "\u{2030}"),
        ("prime", "\u{2032}"),
        ("Prime", "\u{2033}"),
        ("infin", "\u{221E}"),
        ("ne", "\u{2260}"),
        ("le", "\u{2264}"),
        ("ge", "\u{2265}"),
        ("larr", "\u{2190}"),
        ("uarr", "\u{2191}"),
        ("rarr", "\u{2192}"),
        ("darr", "\u{2193}"),
        ("harr", "\u{2194}"),
        ("lArr", "\u{21D0}"),
        ("rArr", "\u{21D2}"),
        ("hArr", "\u{21D4}"),
        ("spades", "\u{2660}"),
        ("clubs", "\u{2663}"),
        ("hearts", "\u{2665}"),
        ("diams", "\u{2666}"),
        ("forall", "\u{2200}"),
        ("exist", "\u{2203}"),
        ("empty", "\u{2205}"),
        ("isin", "\u{2208}"),
        ("sum", "\u{2211}"),
        ("prod", "\u{220F}"),
        ("minus", "\u{2212}"),
        ("int", "\u{222B}"),
        ("part", "\u{2202}"),
        ("nabla", "\u{2207}"),
        ("prop", "\u{221D}"),
        ("propto", "\u{221D}"),
        ("infin", "\u{221E}"),
        ("ang", "\u{2220}"),
        ("perp", "\u{22A5}"),
        ("sdot", "\u{22C5}"),
        ("or", "\u{2228}"),
        ("and", "\u{2227}"),
        ("not", "\u{00AC}"),
        ("cap", "\u{2229}"),
        ("cup", "\u{222A}"),
        ("sub", "\u{2282}"),
        ("sup", "\u{2283}"),
        ("equiv", "\u{2261}"),
        ("sim", "\u{223C}"),
        ("cong", "\u{2245}"),
        ("asymp", "\u{2248}"),
        ("radic", "\u{221A}"),
        ("fnof", "\u{0192}"),
        ("emsp", "\u{2003}"),
        ("ensp", "\u{2002}"),
        ("thinsp", "\u{2009}"),
        ("zwnj", "\u{200C}"),
        ("zwj", "\u{200D}"),
        ("lrm", "\u{200E}"),
        ("rlm", "\u{200F}"),
    ];
    entries.iter().cloned().collect()
}

/// A small extension trait that lets us mutate the kind inside `NodeKind`.
/// We use it in `consume_start_tag` to set attributes after building the node.
#[allow(dead_code)]
trait _Unused {}

