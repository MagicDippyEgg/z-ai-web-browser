//! A small but real CSS parser.
//!
//! Supports:
//!  * At-rules: `@import`, `@media` (treated as transparent wrappers), `@keyframes`, `@font-face`
//!  * Style rules with one or more comma-separated selectors
//!  * Property declarations with `!important`
//!  * Comments
//!  * Strings, urls, numbers, percentages, lengths, colors, identifiers
//!  * Functions: `rgb(...)`, `rgba(...)`, `hsl(...)`, `url(...)`, `calc(...)` (calc is parsed but not evaluated)
//!
//! Does not support full CSS custom properties cascading semantics, container
//! queries, or `@layer`. We treat unknown at-rules as best-effort skip blocks.

use super::selector::Selector;
use smol_str::SmolStr;
use std::fmt;

/// A parsed stylesheet.
#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
pub enum Rule {
    Style(StyleRule),
    Media(MediaRule),
    Import(String),
    FontFace(FontFaceRule),
    Keyframes(KeyframesRule),
    Other(OtherRule),
}

#[derive(Debug, Clone)]
pub struct StyleRule {
    pub selectors: Vec<Selector>,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone)]
pub struct MediaRule {
    pub query: String,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Default)]
pub struct FontFaceRule {
    pub family: Option<SmolStr>,
    pub src: Option<SmolStr>,
    pub weight: Option<u16>,
    pub style: Option<SmolStr>,
}

#[derive(Debug, Clone)]
pub struct KeyframesRule {
    pub name: SmolStr,
    pub frames: Vec<(String, Vec<Declaration>)>,
}

#[derive(Debug, Clone)]
pub struct OtherRule {
    pub name: String,
    pub prelude: String,
    pub body: Option<Vec<Rule>>,
}

#[derive(Debug, Clone)]
pub struct Declaration {
    pub property: SmolStr,
    pub value: Value,
    pub important: bool,
}

/// A CSS value. We keep this loose: most values are stored as a token stream
/// (`Value::List`) so we can handle things like `1px solid #f00` uniformly.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Ident(SmolStr),
    String(SmolStr),
    Number(f64),
    Percentage(f64),
    Length(f64, Unit),
    Angle(f64, AngleUnit),
    Time(f64),
    Url(SmolStr),
    Color(u32),
    Function(SmolStr, Vec<Value>),
    List(Vec<Value>),
    Variable(SmolStr),
    Initial,
    Inherit,
    Unset,
    None,
    Auto,
    /// Slash-separated values (used for `font: 16px/24px sans-serif`).
    Slash(Vec<Value>),
}

impl Value {
    pub fn to_string_raw(&self) -> String {
        match self {
            Value::Ident(s) => s.to_string(),
            Value::String(s) => format!("\"{}\"", s),
            Value::Number(n) => {
                if *n == n.trunc() {
                    format!("{}", *n as i64)
                } else {
                    format!("{}", n)
                }
            }
            Value::Percentage(p) => format!("{}%", p),
            Value::Length(v, u) => format!("{}{}", v, u.as_str()),
            Value::Angle(v, u) => format!("{}{}", v, u.as_str()),
            Value::Time(t) => format!("{}ms", t),
            Value::Url(u) => format!("url({})", u),
            Value::Color(c) => format!("#{:08x}", c),
            Value::Function(name, args) => {
                let args_str = args
                    .iter()
                    .map(|a| a.to_string_raw())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({})", name, args_str)
            }
            Value::List(items) => items
                .iter()
                .map(|v| v.to_string_raw())
                .collect::<Vec<_>>()
                .join(" "),
            Value::Variable(name) => format!("var({})", name),
            Value::Initial => "initial".to_string(),
            Value::Inherit => "inherit".to_string(),
            Value::Unset => "unset".to_string(),
            Value::None => "none".to_string(),
            Value::Auto => "auto".to_string(),
            Value::Slash(items) => items
                .iter()
                .map(|v| v.to_string_raw())
                .collect::<Vec<_>>()
                .join("/"),
        }
    }

    pub fn as_color(&self) -> Option<u32> {
        match self {
            Value::Color(c) => Some(*c),
            _ => None,
        }
    }

    pub fn as_length_px(&self) -> Option<f64> {
        match self {
            Value::Length(v, Unit::Px) => Some(*v),
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::Percentage(p) => Some(*p),
            _ => None,
        }
    }

    pub fn as_ident(&self) -> Option<&str> {
        match self {
            Value::Ident(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            Value::String(s) | Value::Ident(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Px,
    Em,
    Rem,
    Vw,
    Vh,
    Vmin,
    Vmax,
    Pt,
    Pc,
    In,
    Cm,
    Mm,
    Ex,
    Ch,
    Fr,
    Deg,
    Rad,
    Turn,
    Grad,
    Hz,
    Khz,
    Dpi,
    Dpcm,
    Dppx,
    S,
    Ms,
    Percent,
}

impl Unit {
    fn as_str(&self) -> &'static str {
        match self {
            Unit::Px => "px",
            Unit::Em => "em",
            Unit::Rem => "rem",
            Unit::Vw => "vw",
            Unit::Vh => "vh",
            Unit::Vmin => "vmin",
            Unit::Vmax => "vmax",
            Unit::Pt => "pt",
            Unit::Pc => "pc",
            Unit::In => "in",
            Unit::Cm => "cm",
            Unit::Mm => "mm",
            Unit::Ex => "ex",
            Unit::Ch => "ch",
            Unit::Fr => "fr",
            Unit::Deg => "deg",
            Unit::Rad => "rad",
            Unit::Turn => "turn",
            Unit::Grad => "grad",
            Unit::Hz => "Hz",
            Unit::Khz => "kHz",
            Unit::Dpi => "dpi",
            Unit::Dpcm => "dpcm",
            Unit::Dppx => "dppx",
            Unit::S => "s",
            Unit::Ms => "ms",
            Unit::Percent => "%",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AngleUnit {
    Deg,
    Rad,
    Turn,
    Grad,
}

impl AngleUnit {
    fn as_str(&self) -> &'static str {
        match self {
            AngleUnit::Deg => "deg",
            AngleUnit::Rad => "rad",
            AngleUnit::Turn => "turn",
            AngleUnit::Grad => "grad",
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_raw())
    }
}

// ===================== Parser =====================

pub fn parse(input: &str) -> Stylesheet {
    let mut p = ParserImpl::new(input);
    p.parse_stylesheet()
}

struct ParserImpl<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> ParserImpl<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            src: input.as_bytes(),
            pos: 0,
        }
    }

    fn parse_stylesheet(&mut self) -> Stylesheet {
        let mut rules = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.pos >= self.src.len() {
                break;
            }
            if self.peek() == Some(b'}') {
                // Stray close brace — stop or skip.
                self.pos += 1;
                continue;
            }
            if self.peek() == Some(b'@') {
                if let Some(rule) = self.parse_at_rule() {
                    rules.push(rule);
                }
            } else if let Some(rule) = self.parse_style_rule() {
                rules.push(Rule::Style(rule));
            } else {
                // Stuck — advance one byte to avoid infinite loop.
                self.pos += 1;
            }
        }
        Stylesheet { rules }
    }

    fn parse_at_rule(&mut self) -> Option<Rule> {
        self.pos += 1; // @
        let name = self.consume_ident();
        self.skip_whitespace_and_comments();
        // Read prelude up to '{' or ';'
        let mut prelude = String::new();
        while let Some(b) = self.peek() {
            if b == b'{' || b == b';' {
                break;
            }
            prelude.push(b as char);
            self.pos += 1;
        }
        let prelude = prelude.trim().to_string();
        match name.as_str() {
            "import" => {
                // Skip the ';'
                if self.peek() == Some(b';') {
                    self.pos += 1;
                }
                // prelude is `url("...")` or `"..."` possibly with media query
                let url = prelude
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_matches('"')
                    .to_string();
                if url.starts_with("url(") {
                    let u = url
                        .trim_start_matches("url(")
                        .trim_end_matches(')')
                        .trim_matches(|c: char| c == '"' || c == '\'')
                        .to_string();
                    return Some(Rule::Import(u));
                }
                return Some(Rule::Import(url));
            }
            "media" => {
                if self.peek() == Some(b'{') {
                    self.pos += 1;
                    let mut inner = Vec::new();
                    loop {
                        self.skip_whitespace_and_comments();
                        if self.peek() == Some(b'}') || self.pos >= self.src.len() {
                            break;
                        }
                        if self.peek() == Some(b'@') {
                            if let Some(r) = self.parse_at_rule() {
                                inner.push(r);
                            }
                        } else if let Some(r) = self.parse_style_rule() {
                            inner.push(Rule::Style(r));
                        } else {
                            self.pos += 1;
                        }
                    }
                    if self.peek() == Some(b'}') {
                        self.pos += 1;
                    }
                    return Some(Rule::Media(MediaRule {
                        query: prelude,
                        rules: inner,
                    }));
                }
            }
            "font-face" => {
                if self.peek() == Some(b'{') {
                    self.pos += 1;
                    let decls = self.parse_declarations();
                    let mut ff = FontFaceRule::default();
                    for d in &decls {
                        match d.property.as_str() {
                            "font-family" => {
                                ff.family = d.value.as_string().map(SmolStr::new);
                            }
                            "src" => ff.src = d.value.as_string().map(SmolStr::new),
                            "font-weight" => {
                                if let Some(n) = d.value.as_number() {
                                    ff.weight = Some(n as u16);
                                }
                            }
                            "font-style" => ff.style = d.value.as_string().map(SmolStr::new),
                            _ => {}
                        }
                    }
                    return Some(Rule::FontFace(ff));
                }
            }
            "keyframes" | "-webkit-keyframes" => {
                let name = SmolStr::new(prelude.split_whitespace().next().unwrap_or(""));
                if self.peek() == Some(b'{') {
                    self.pos += 1;
                    let mut frames = Vec::new();
                    loop {
                        self.skip_whitespace_and_comments();
                        if self.peek() == Some(b'}') || self.pos >= self.src.len() {
                            break;
                        }
                        // Selector: "0%", "100%", "from", "to", or "0%, 50%"
                        let mut sel = String::new();
                        while let Some(b) = self.peek() {
                            if b == b'{' {
                                break;
                            }
                            sel.push(b as char);
                            self.pos += 1;
                        }
                        let sel = sel.trim().to_string();
                        if self.peek() == Some(b'{') {
                            self.pos += 1;
                            let decls = self.parse_declarations();
                            frames.push((sel, decls));
                        }
                    }
                    if self.peek() == Some(b'}') {
                        self.pos += 1;
                    }
                    return Some(Rule::Keyframes(KeyframesRule { name, frames }));
                }
            }
            _ => {
                // Unknown at-rule. Skip its body if present.
                if self.peek() == Some(b'{') {
                    self.pos += 1;
                    let depth = 1;
                    let mut d = depth;
                    while d > 0 && self.pos < self.src.len() {
                        let b = self.src[self.pos];
                        self.pos += 1;
                        if b == b'{' {
                            d += 1;
                        } else if b == b'}' {
                            d -= 1;
                        }
                    }
                } else if self.peek() == Some(b';') {
                    self.pos += 1;
                }
                return Some(Rule::Other(OtherRule {
                    name,
                    prelude,
                    body: None,
                }));
            }
        }
        None
    }

    fn parse_style_rule(&mut self) -> Option<StyleRule> {
        // Selector list up to '{'
        let mut sel_str = String::new();
        let mut depth = 0;
        while let Some(b) = self.peek() {
            if b == b'{' && depth == 0 {
                break;
            }
            if b == b'(' {
                depth += 1;
            }
            if b == b')' {
                depth -= 1;
            }
            if b == b'}' {
                // Premature end
                return None;
            }
            sel_str.push(b as char);
            self.pos += 1;
        }
        if self.peek() != Some(b'{') {
            return None;
        }
        self.pos += 1;
        let selectors: Vec<Selector> = sel_str
            .split(',')
            .filter_map(|s| Selector::parse(s.trim()))
            .collect();
        if selectors.is_empty() {
            // Skip block
            self.skip_until_close_brace();
            return None;
        }
        let declarations = self.parse_declarations();
        Some(StyleRule {
            selectors,
            declarations,
        })
    }

    fn parse_declarations(&mut self) -> Vec<Declaration> {
        let mut out = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.peek() == Some(b'}') || self.pos >= self.src.len() {
                break;
            }
            // Property name
            let p_start = self.pos;
            while let Some(b) = self.peek() {
                if b == b':' || b == b';' || b == b'}' || b == b'{' {
                    break;
                }
                self.pos += 1;
            }
            let property = std::str::from_utf8(&self.src[p_start..self.pos])
                .unwrap_or("")
                .trim()
                .to_string();
            if property.is_empty() {
                if self.peek() == Some(b';') {
                    self.pos += 1;
                }
                continue;
            }
            if self.peek() == Some(b':') {
                self.pos += 1;
            }
            self.skip_whitespace_and_comments();
            // Value up to ';' or '}' (respecting parens)
            let mut depth = 0;
            let v_start = self.pos;
            while let Some(b) = self.peek() {
                if depth == 0 && (b == b';' || b == b'}') {
                    break;
                }
                if b == b'(' {
                    depth += 1;
                }
                if b == b')' {
                    depth -= 1;
                }
                self.pos += 1;
            }
            let value_str = std::str::from_utf8(&self.src[v_start..self.pos])
                .unwrap_or("")
                .trim()
                .to_string();
            let mut important = false;
            let value_str = if let Some(stripped) = value_str
                .strip_suffix("!important")
                .or_else(|| value_str.strip_suffix("! important"))
            {
                important = true;
                stripped.trim().to_string()
            } else {
                value_str
            };
            let value = parse_value(&value_str);
            out.push(Declaration {
                property: SmolStr::new(property.to_ascii_lowercase()),
                value,
                important,
            });
            if self.peek() == Some(b';') {
                self.pos += 1;
            }
        }
        if self.peek() == Some(b'}') {
            self.pos += 1;
        }
        out
    }

    fn skip_until_close_brace(&mut self) {
        let mut d = 1;
        while d > 0 && self.pos < self.src.len() {
            let b = self.src[self.pos];
            self.pos += 1;
            if b == b'{' {
                d += 1;
            } else if b == b'}' {
                d -= 1;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            while let Some(b) = self.peek() {
                if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if self.pos + 1 < self.src.len() && self.src[self.pos] == b'/' && self.src[self.pos + 1] == b'*' {
                self.pos += 2;
                while self.pos + 1 < self.src.len() {
                    if self.src[self.pos] == b'*' && self.src[self.pos + 1] == b'/' {
                        self.pos += 2;
                        break;
                    }
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn consume_ident(&mut self) -> String {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b >= 0x80 {
                self.pos += 1;
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.src[start..self.pos]).into_owned()
    }
}

/// Parse a CSS value string into a `Value`. This is the public entry-point
/// used both by the parser and by inline-style parsing (`style="..."`).
pub fn parse_value(s: &str) -> Value {
    let s = s.trim();
    if s.is_empty() {
        return Value::None;
    }
    // Handle slash separator first (font: 16px/24px ...)
    if s.contains('/') && !s.contains('(') {
        let parts: Vec<Value> = s.split('/').map(|p| parse_value(p)).collect();
        if parts.len() > 1 {
            return Value::Slash(parts);
        }
    }
    // Tokens separated by whitespace
    let tokens = tokenize_value(s);
    if tokens.is_empty() {
        return Value::None;
    }
    if tokens.len() == 1 {
        return tokens.into_iter().next().unwrap();
    }
    Value::List(tokens)
}

fn tokenize_value(s: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip whitespace
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // Function call
        let name_start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
        {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'(' && i > name_start {
            let name = std::str::from_utf8(&bytes[name_start..i]).unwrap_or("");
            i += 1;
            // Read until matching ')'
            let mut depth = 1;
            let arg_start = i;
            while i < bytes.len() && depth > 0 {
                if bytes[i] == b'(' {
                    depth += 1;
                } else if bytes[i] == b')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                i += 1;
            }
            let arg_str = std::str::from_utf8(&bytes[arg_start..i]).unwrap_or("");
            if i < bytes.len() {
                i += 1; // ')'
            }
            let args: Vec<Value> = arg_str
                .split(',')
                .map(|a| parse_value(a.trim()))
                .collect();
            let val = match name {
                "rgb" | "rgba" => parse_color_func(arg_str, false),
                "hsl" | "hsla" => parse_color_func(arg_str, true),
                "url" => {
                    let u = arg_str.trim().trim_matches(|c: char| c == '"' || c == '\'');
                    Value::Url(SmolStr::new(u))
                }
                "var" => {
                    let name = arg_str.split(',').next().unwrap_or("").trim();
                    Value::Variable(SmolStr::new(name.trim_start_matches("--")))
                }
                "calc" => Value::Function(SmolStr::new("calc"), args),
                _ => Value::Function(SmolStr::new(name), args),
            };
            out.push(val);
            continue;
        }
        // Reset — not a function
        i = name_start;
        // String
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let q = bytes[i];
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != q {
                i += 1;
            }
            let s = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
            if i < bytes.len() {
                i += 1;
            }
            out.push(Value::String(SmolStr::new(s)));
            continue;
        }
        // Hash color
        if bytes[i] == b'#' {
            let start = i + 1;
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            let hex = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
            if let Some(c) = parse_hex_color(hex) {
                out.push(Value::Color(c));
            } else {
                out.push(Value::Ident(SmolStr::new(format!("#{}", hex))));
            }
            continue;
        }
        // Number / length / percentage
        if bytes[i] == b'-' || bytes[i] == b'+' || bytes[i].is_ascii_digit() || bytes[i] == b'.' {
            let start = i;
            // Sign
            if bytes[i] == b'-' || bytes[i] == b'+' {
                i += 1;
            }
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            // Optional exponent — only if 'e'/'E' is followed by a digit or +/-digit
            if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                // Check if this is actually an exponent (e.g. 1e5) and not
                // the start of a unit like 'em' or 'ex'. An exponent must be
                // followed by an optional sign and at least one digit.
                let next = bytes.get(i + 1).copied();
                let is_exponent = match next {
                    Some(b'-') | Some(b'+') => bytes.get(i + 2).map(|b| b.is_ascii_digit()).unwrap_or(false),
                    Some(d) if d.is_ascii_digit() => true,
                    _ => false,
                };
                if is_exponent {
                    i += 1;
                    if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
                        i += 1;
                    }
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let num_str = std::str::from_utf8(&bytes[start..i]).unwrap_or("0");
            let num: f64 = num_str.parse().unwrap_or(0.0);
            // Unit?
            let unit_start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i > unit_start {
                let unit_str = std::str::from_utf8(&bytes[unit_start..i]).unwrap_or("");
                match unit_str {
                    "px" => out.push(Value::Length(num, Unit::Px)),
                    "em" => out.push(Value::Length(num, Unit::Em)),
                    "rem" => out.push(Value::Length(num, Unit::Rem)),
                    "vw" => out.push(Value::Length(num, Unit::Vw)),
                    "vh" => out.push(Value::Length(num, Unit::Vh)),
                    "vmin" => out.push(Value::Length(num, Unit::Vmin)),
                    "vmax" => out.push(Value::Length(num, Unit::Vmax)),
                    "pt" => out.push(Value::Length(num, Unit::Pt)),
                    "pc" => out.push(Value::Length(num, Unit::Pc)),
                    "in" => out.push(Value::Length(num, Unit::In)),
                    "cm" => out.push(Value::Length(num, Unit::Cm)),
                    "mm" => out.push(Value::Length(num, Unit::Mm)),
                    "ex" => out.push(Value::Length(num, Unit::Ex)),
                    "ch" => out.push(Value::Length(num, Unit::Ch)),
                    "fr" => out.push(Value::Length(num, Unit::Fr)),
                    "deg" => out.push(Value::Angle(num, AngleUnit::Deg)),
                    "rad" => out.push(Value::Angle(num, AngleUnit::Rad)),
                    "turn" => out.push(Value::Angle(num, AngleUnit::Turn)),
                    "grad" => out.push(Value::Angle(num, AngleUnit::Grad)),
                    "%" => out.push(Value::Percentage(num)),
                    "s" => out.push(Value::Time(num * 1000.0)),
                    "ms" => out.push(Value::Time(num)),
                    _ => out.push(Value::Ident(SmolStr::new(format!("{}{}", num, unit_str)))),
                }
            } else {
                out.push(Value::Number(num));
            }
            continue;
        }
        // Identifier
        let start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
        {
            i += 1;
        }
        if i > start {
            let ident = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
            let val = match ident {
                "initial" => Value::Initial,
                "inherit" => Value::Inherit,
                "unset" => Value::Unset,
                "none" => Value::None,
                "auto" => Value::Auto,
                _ => {
                    // Try color name
                    if let Some(c) = named_color(ident) {
                        Value::Color(c)
                    } else {
                        Value::Ident(SmolStr::new(ident))
                    }
                }
            };
            out.push(val);
            continue;
        }
        // Skip unknown char
        i += 1;
    }
    out
}

fn parse_color_func(args: &str, hsl: bool) -> Value {
    let parts: Vec<&str> = args.split(|c: char| c == ',' || c == ' ').collect();
    let nums: Vec<f64> = parts
        .iter()
        .filter_map(|s| s.trim().trim_end_matches('%').parse::<f64>().ok())
        .collect();
    if hsl {
        if nums.len() >= 3 {
            let h = nums[0] / 360.0;
            let s = nums[1] / 100.0;
            let l = nums[2] / 100.0;
            let a = nums.get(3).copied().unwrap_or(1.0);
            let (r, g, b) = hsl_to_rgb(h, s, l);
            return Value::Color(rgba(r, g, b, a));
        }
    } else if nums.len() >= 3 {
        let r = nums[0] as u8;
        let g = nums[1] as u8;
        let b = nums[2] as u8;
        let a = nums.get(3).copied().unwrap_or(1.0);
        return Value::Color(rgba(r, g, b, a));
    }
    Value::Function(SmolStr::new(if hsl { "hsl" } else { "rgb" }), Vec::new())
}

fn parse_hex_color(hex: &str) -> Option<u32> {
    let (r, g, b, a) = match hex.len() {
        3 => {
            let r = u8::from_str_radix(&format!("{}{}", &hex[0..1], &hex[0..1]), 16).ok()?;
            let g = u8::from_str_radix(&format!("{}{}", &hex[1..2], &hex[1..2]), 16).ok()?;
            let b = u8::from_str_radix(&format!("{}{}", &hex[2..3], &hex[2..3]), 16).ok()?;
            (r, g, b, 255u8)
        }
        4 => {
            let r = u8::from_str_radix(&format!("{}{}", &hex[0..1], &hex[0..1]), 16).ok()?;
            let g = u8::from_str_radix(&format!("{}{}", &hex[1..2], &hex[1..2]), 16).ok()?;
            let b = u8::from_str_radix(&format!("{}{}", &hex[2..3], &hex[2..3]), 16).ok()?;
            let a = u8::from_str_radix(&format!("{}{}", &hex[3..4], &hex[3..4]), 16).ok()?;
            (r, g, b, a)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            (r, g, b, 255u8)
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            (r, g, b, a)
        }
        _ => return None,
    };
    Some(rgba(r, g, b, a as f64 / 255.0))
}

fn rgba(r: u8, g: u8, b: u8, a: f64) -> u32 {
    let a = (a.clamp(0.0, 1.0) * 255.0) as u8;
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    if s == 0.0 {
        let v = (l * 255.0) as u8;
        return (v, v, v);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue_to_rgb = |p: f64, q: f64, t: f64| -> f64 {
        let mut t = t;
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 1.0 / 2.0 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    let r = hue_to_rgb(p, q, h + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0 / 3.0);
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

pub fn named_color(name: &str) -> Option<u32> {
    let n = name.to_ascii_lowercase();
    // A useful subset of CSS named colors. The full list is huge; we include
    // the most common ones plus the basic 16.
    let hex: Option<&str> = match n.as_str() {
        "black" => Some("000000"),
        "white" => Some("ffffff"),
        "red" => Some("ff0000"),
        "green" => Some("008000"),
        "lime" => Some("00ff00"),
        "blue" => Some("0000ff"),
        "yellow" => Some("ffff00"),
        "cyan" | "aqua" => Some("00ffff"),
        "magenta" | "fuchsia" => Some("ff00ff"),
        "silver" => Some("c0c0c0"),
        "gray" | "grey" => Some("808080"),
        "maroon" => Some("800000"),
        "olive" => Some("808000"),
        "purple" => Some("800080"),
        "teal" => Some("008080"),
        "navy" => Some("000080"),
        "orange" => Some("ffa500"),
        "pink" => Some("ffc0cb"),
        "brown" => Some("a52a2a"),
        "gold" => Some("ffd700"),
        "tomato" => Some("ff6347"),
        "crimson" => Some("dc143c"),
        "indigo" => Some("4b0082"),
        "violet" => Some("ee82ee"),
        "khaki" => Some("f0e68c"),
        "salmon" => Some("fa8072"),
        "coral" => Some("ff7f50"),
        "wheat" => Some("f5deb3"),
        "tan" => Some("d2b48c"),
        "azure" => Some("f0ffff"),
        "ivory" => Some("fffff0"),
        "lavender" => Some("e6e6fa"),
        "beige" => Some("f5f5dc"),
        "mintcream" => Some("f5fffa"),
        "snow" => Some("fffafa"),
        "lightgray" | "lightgrey" => Some("d3d3d3"),
        "darkgray" | "darkgrey" => Some("a9a9a9"),
        "dimgray" | "dimgrey" => Some("696969"),
        "slategray" => Some("708090"),
        "lightslategray" | "lightslategrey" => Some("778899"),
        "lightblue" => Some("add8e6"),
        "lightgreen" => Some("90ee90"),
        "lightyellow" => Some("ffffe0"),
        "lightpink" => Some("ffb6c1"),
        "lightsalmon" => Some("ffa07a"),
        "lightcoral" => Some("f08080"),
        "lightcyan" => Some("e0ffff"),
        "darkblue" => Some("00008b"),
        "darkgreen" => Some("006400"),
        "darkred" => Some("8b0000"),
        "darkorange" => Some("ff8c00"),
        "darkviolet" => Some("9400d3"),
        "darkcyan" => Some("008b8b"),
        "darkmagenta" => Some("8b008b"),
        "darkgoldenrod" => Some("b8860b"),
        "royalblue" => Some("4169e1"),
        "mediumblue" => Some("0000cd"),
        "midnightblue" => Some("191970"),
        "steelblue" => Some("4682b4"),
        "skyblue" => Some("87ceeb"),
        "deepskyblue" => Some("00bfff"),
        "dodgerblue" => Some("1e90ff"),
        "forestgreen" => Some("228b22"),
        "seagreen" => Some("2e8b57"),
        "springgreen" => Some("00ff7f"),
        "lawngreen" => Some("7cfc00"),
        "chartreuse" => Some("7fff00"),
        "darkkhaki" => Some("bdb76b"),
        "peru" => Some("cd853f"),
        "chocolate" => Some("d2691e"),
        "sienna" => Some("a0522d"),
        "firebrick" => Some("b22222"),
        "rosybrown" => Some("bc8f8f"),
        "indianred" => Some("cd5c5c"),
        "lightseagreen" => Some("20b2aa"),
        "mediumturquoise" => Some("48d1cc"),
        "turquoise" => Some("40e0d0"),
        "paleturquoise" => Some("afeeee"),
        "aquamarine" => Some("7fffd4"),
        "mediumaquamarine" => Some("66cdaa"),
        "mediumseagreen" => Some("3cb371"),
        "mediumspringgreen" => Some("00fa9a"),
        "rebeccapurple" => Some("663399"),
        "transparent" => Some("00000000"),
        _ => None,
    };
    hex.and_then(parse_hex_color)
}
