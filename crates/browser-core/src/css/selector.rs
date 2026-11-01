//! CSS selectors and specificity.
//!
//! Supports the subset of selectors that real-world web pages rely on:
//!
//!  * Type:        `div`
//!  * Universal:   `*`
//!  * Class:       `.foo`
//!  * Id:          `#bar`
//!  * Attribute:   `[type=text]`, `[href^="https"]`, `[disabled]`
//!  * Pseudo-class: `:hover`, `:focus`, `:active`, `:first-child`, `:last-child`,
//!                  `:nth-child(n)`, `:not(...)`, `:empty`, `:checked`
//!  * Pseudo-element: `::before`, `::after` (matched but no actual content gen)
//!  * Combinators: descendant (whitespace), child (`>`), adjacent sibling (`+`),
//!                  general sibling (`~`)
//!  * Comma-separated selector lists
//!
//! We do **not** support `:nth-child(odd)` formulas other than simple integer
//! values, structural pseudo-classes beyond first/last child, or `:has()`.

use crate::html::dom::{Node, NodeKind, NodeRef};
use smol_str::SmolStr;
use std::fmt;

#[derive(Debug, Clone)]
pub struct Selector {
    pub compound: Vec<SimpleSelector>,
    pub combinators: Vec<Combinator>,
    /// Pre-computed specificity (a, b, c).
    pub specificity: (u32, u32, u32),
}

#[derive(Debug, Clone)]
pub struct Combinator {
    pub kind: CombinatorKind,
    pub next: Vec<SimpleSelector>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombinatorKind {
    Descendant,
    Child,
    AdjacentSibling,
    GeneralSibling,
}

#[derive(Debug, Clone)]
pub enum SimpleSelector {
    Universal,
    Type(SmolStr),
    Class(SmolStr),
    Id(SmolStr),
    Attribute {
        name: SmolStr,
        op: AttrOp,
        value: Option<SmolStr>,
    },
    PseudoClass(PseudoClass),
    PseudoElement(SmolStr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrOp {
    /// `[attr]` — just presence
    Present,
    /// `[attr=val]`
    Equals,
    /// `[attr~=val]` — whitespace-separated list contains val
    Includes,
    /// `[attr|=val]` — equals or starts with `val-`
    DashMatch,
    /// `[attr^=val]` — starts with
    PrefixMatch,
    /// `[attr$=val]` — ends with
    SuffixMatch,
    /// `[attr*=val]` — contains
    SubstringMatch,
}

#[derive(Debug, Clone)]
pub enum PseudoClass {
    Hover,
    Focus,
    Active,
    Visited,
    Link,
    FirstChild,
    LastChild,
    OnlyChild,
    Empty,
    Checked,
    Disabled,
    Enabled,
    NthChild(Option<i32>),
    Not(Box<Selector>),
    Root,
    Other(SmolStr),
}

impl Selector {
    pub fn parse(input: &str) -> Option<Selector> {
        let input = input.trim();
        if input.is_empty() {
            return None;
        }
        let mut parser = SelectorParser::new(input);
        parser.parse_complex()
    }

    /// Test whether this selector matches the given node, when rooted at the
    /// given document (used to walk up ancestors for combinators).
    pub fn matches(&self, node: &NodeRef) -> bool {
        // The last compound in the chain is the "subject" — it must match `node`.
        // We then walk leftwards, applying combinators.
        let n = self.compound.len();
        let last_compound = if n == 0 {
            return false;
        } else {
            &self.compound
        };
        if !compound_matches(last_compound, node) {
            return false;
        }
        // Walk combinators right-to-left.
        let mut current = node.clone();
        for comb in self.combinators.iter().rev() {
            let next = &comb.next;
            let matched = match comb.kind {
                CombinatorKind::Descendant => {
                    let mut ancestor = current.parent();
                    let mut found = false;
                    while let Some(a) = ancestor {
                        if compound_matches(next, &a) {
                            current = a.clone();
                            found = true;
                            break;
                        }
                        ancestor = a.parent();
                    }
                    found
                }
                CombinatorKind::Child => {
                    if let Some(p) = current.parent() {
                        if compound_matches(next, &p) {
                            current = p.clone();
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                CombinatorKind::AdjacentSibling => {
                    if let Some(prev) = current.previous_sibling() {
                        if compound_matches(next, &prev) {
                            current = prev.clone();
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                CombinatorKind::GeneralSibling => {
                    let mut sibling = current.previous_sibling();
                    let mut found = false;
                    while let Some(s) = sibling {
                        if compound_matches(next, &s) {
                            current = s.clone();
                            found = true;
                            break;
                        }
                        sibling = s.previous_sibling();
                    }
                    found
                }
            };
            if !matched {
                return false;
            }
        }
        true
    }
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for s in &self.compound {
            write!(f, "{}", s)?;
        }
        for c in &self.combinators {
            match c.kind {
                CombinatorKind::Descendant => write!(f, " ")?,
                CombinatorKind::Child => write!(f, " > ")?,
                CombinatorKind::AdjacentSibling => write!(f, " + ")?,
                CombinatorKind::GeneralSibling => write!(f, " ~ ")?,
            }
            for s in &c.next {
                write!(f, "{}", s)?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for SimpleSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimpleSelector::Universal => write!(f, "*"),
            SimpleSelector::Type(t) => write!(f, "{}", t),
            SimpleSelector::Class(c) => write!(f, ".{}", c),
            SimpleSelector::Id(i) => write!(f, "#{}", i),
            SimpleSelector::Attribute { name, op, value } => {
                write!(f, "[{}", name)?;
                if *op != AttrOp::Present {
                    let v = value.as_ref().map(|s| s.as_str()).unwrap_or("");
                    let sym = match op {
                        AttrOp::Equals => "=",
                        AttrOp::Includes => "~=",
                        AttrOp::DashMatch => "|=",
                        AttrOp::PrefixMatch => "^=",
                        AttrOp::SuffixMatch => "$=",
                        AttrOp::SubstringMatch => "*=",
                        AttrOp::Present => "",
                    };
                    write!(f, "{}\"{}\"", sym, v)?;
                }
                write!(f, "]")
            }
            SimpleSelector::PseudoClass(p) => match p {
                PseudoClass::Hover => write!(f, ":hover"),
                PseudoClass::Focus => write!(f, ":focus"),
                PseudoClass::Active => write!(f, ":active"),
                PseudoClass::Visited => write!(f, ":visited"),
                PseudoClass::Link => write!(f, ":link"),
                PseudoClass::FirstChild => write!(f, ":first-child"),
                PseudoClass::LastChild => write!(f, ":last-child"),
                PseudoClass::OnlyChild => write!(f, ":only-child"),
                PseudoClass::Empty => write!(f, ":empty"),
                PseudoClass::Checked => write!(f, ":checked"),
                PseudoClass::Disabled => write!(f, ":disabled"),
                PseudoClass::Enabled => write!(f, ":enabled"),
                PseudoClass::NthChild(Some(n)) => write!(f, ":nth-child({})", n),
                PseudoClass::NthChild(None) => write!(f, ":nth-child(n)"),
                PseudoClass::Not(_) => write!(f, ":not(...)"),
                PseudoClass::Root => write!(f, ":root"),
                PseudoClass::Other(s) => write!(f, ":{}", s),
            },
            SimpleSelector::PseudoElement(p) => write!(f, "::{}", p),
        }
    }
}

fn compound_matches(compound: &[SimpleSelector], node: &NodeRef) -> bool {
    for s in compound {
        if !simple_matches(s, node) {
            return false;
        }
    }
    true
}

fn simple_matches(s: &SimpleSelector, node: &NodeRef) -> bool {
    let kind = node.kind();
    let elem = match &kind {
        NodeKind::Element(e) => e,
        _ => return false,
    };
    match s {
        SimpleSelector::Universal => true,
        SimpleSelector::Type(t) => elem.tag.eq_ignore_ascii_case(t),
        SimpleSelector::Class(c) => elem.classes().iter().any(|x| x == c),
        SimpleSelector::Id(i) => elem.get_attr("id").map(|v| v == i).unwrap_or(false),
        SimpleSelector::Attribute { name, op, value } => {
            let attr = elem.get_attr(name);
            match attr {
                None => false,
                Some(actual) => {
                    let v = value.as_ref().map(|s| s.as_str()).unwrap_or("");
                    match op {
                        AttrOp::Present => true,
                        AttrOp::Equals => actual == v,
                        AttrOp::Includes => actual.split_whitespace().any(|p| p == v),
                        AttrOp::DashMatch => actual == v || actual.starts_with(&format!("{}-", v)),
                        AttrOp::PrefixMatch => actual.starts_with(v),
                        AttrOp::SuffixMatch => actual.ends_with(v),
                        AttrOp::SubstringMatch => actual.contains(v),
                    }
                }
            }
        }
        SimpleSelector::PseudoClass(p) => match p {
            PseudoClass::Hover | PseudoClass::Focus | PseudoClass::Active => {
                // Interactive states are tracked per-page. For matching during
                // cascade we return false — these are evaluated separately at
                // hit-test time.
                false
            }
            PseudoClass::Visited | PseudoClass::Link => {
                elem.tag.eq_ignore_ascii_case("a") && elem.get_attr("href").is_some()
            }
            PseudoClass::FirstChild => {
                if let Some(p) = node.parent() {
                    let mut found_self = false;
                    for c in p.children() {
                        if c.is_element() {
                            if Arc::ptr_eq(&c, node) {
                                found_self = true;
                                break;
                            } else {
                                return false;
                            }
                        }
                    }
                    found_self
                } else {
                    false
                }
            }
            PseudoClass::LastChild => {
                if let Some(p) = node.parent() {
                    let children: Vec<_> = p.children().into_iter().filter(|c| c.is_element()).collect();
                    children.last().map(|c| Arc::ptr_eq(c, node)).unwrap_or(false)
                } else {
                    false
                }
            }
            PseudoClass::OnlyChild => {
                if let Some(p) = node.parent() {
                    let count = p.children().into_iter().filter(|c| c.is_element()).count();
                    count == 1
                } else {
                    false
                }
            }
            PseudoClass::Empty => node.children().is_empty(),
            PseudoClass::Checked => node.checked(),
            PseudoClass::Disabled => elem.get_attr("disabled").is_some(),
            PseudoClass::Enabled => elem.get_attr("disabled").is_none(),
            PseudoClass::NthChild(n) => {
                if let Some(p) = node.parent() {
                    let siblings: Vec<_> = p.children().into_iter().filter(|c| c.is_element()).collect();
                    let idx = siblings.iter().position(|c| Arc::ptr_eq(c, node));
                    if let Some(i) = idx {
                        let i1 = (i + 1) as i32;
                        match n {
                            Some(num) => i1 == *num,
                            None => true, // `:nth-child(n)` matches all
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            PseudoClass::Not(inner) => !inner.matches(node),
            PseudoClass::Root => node.parent().is_none(),
            PseudoClass::Other(_) => false,
        },
        SimpleSelector::PseudoElement(_) => false,
    }
}

use std::sync::Arc;

struct SelectorParser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> SelectorParser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            src: s.as_bytes(),
            pos: 0,
        }
    }

    fn parse_complex(&mut self) -> Option<Selector> {
        let mut compound = self.parse_compound()?;
        let mut combinators = Vec::new();
        let mut specificity = compute_specificity(&compound);
        loop {
            self.skip_ws();
            if self.pos >= self.src.len() {
                break;
            }
            let kind = match self.peek()? {
                b'>' => {
                    self.pos += 1;
                    CombinatorKind::Child
                }
                b'+' => {
                    self.pos += 1;
                    CombinatorKind::AdjacentSibling
                }
                b'~' => {
                    self.pos += 1;
                    CombinatorKind::GeneralSibling
                }
                _ => CombinatorKind::Descendant,
            };
            self.skip_ws();
            let next = self.parse_compound()?;
            let s = compute_specificity(&next);
            specificity.0 += s.0;
            specificity.1 += s.1;
            specificity.2 += s.2;
            combinators.push(Combinator { kind, next });
            compound.len(); // unused
            let _ = compound; // silence
        }
        // The `compound` we accumulated actually is the leftmost; combinators
        // hold the rest. But the way we built it, the leftmost compound was
        // parsed first, then combinators push the right side. So the "subject"
        // (last compound) is `combinators.last().next`, not `compound`.
        // To make `Selector::matches` work as written (where `compound` is the
        // subject), we need to rotate. Let's just re-derive:
        if combinators.is_empty() {
            return Some(Selector {
                compound,
                combinators,
                specificity,
            });
        }
        // Move the last compound to be `compound`, and shift the rest into
        // reverse-ordered combinators. But the matching code expects
        // `combinators` to be in **left-to-right** order (it iterates
        // `.rev()`). So:
        //   compound = rightmost (subject)
        //   combinators[0] = combinator immediately left of subject + its left side
        //   combinators[1] = next-left combinator + its left side
        //   ...
        // We have:
        //   compound = leftmost
        //   combinators[0] = combinator + next compound (going right)
        //   combinators[k] = combinator + next compound
        // We need to rotate.
        let mut all_compounds = vec![compound];
        let mut all_combinators = Vec::new();
        for c in combinators {
            all_compounds.push(c.next);
            all_combinators.push(c.kind);
        }
        // Now `all_compounds[0]` is leftmost, `all_compounds[k]` is rightmost.
        // `all_combinators[i]` is the combinator between compound[i] and compound[i+1].
        let n = all_compounds.len();
        let subject = all_compounds[n - 1].clone();
        let mut new_combinators = Vec::with_capacity(n - 1);
        for i in (0..n - 1).rev() {
            new_combinators.push(Combinator {
                kind: all_combinators[i],
                next: all_compounds[i].clone(),
            });
        }
        Some(Selector {
            compound: subject,
            combinators: new_combinators,
            specificity,
        })
    }

    fn parse_compound(&mut self) -> Option<Vec<SimpleSelector>> {
        let mut out = Vec::new();
        // Optional leading type or universal
        match self.peek()? {
            b'*' => {
                self.pos += 1;
                out.push(SimpleSelector::Universal);
            }
            c if c.is_ascii_alphabetic() => {
                let name = self.consume_ident();
                out.push(SimpleSelector::Type(SmolStr::new(name.to_ascii_lowercase())));
            }
            _ => {}
        }
        // Subsequent selectors: .class, #id, [attr], :pseudo, ::pseudo-element
        loop {
            match self.peek() {
                Some(b'.') => {
                    self.pos += 1;
                    let name = self.consume_ident();
                    out.push(SimpleSelector::Class(SmolStr::new(name)));
                }
                Some(b'#') => {
                    self.pos += 1;
                    let name = self.consume_ident();
                    out.push(SimpleSelector::Id(SmolStr::new(name)));
                }
                Some(b'[') => {
                    self.pos += 1;
                    self.skip_ws();
                    let name = self.consume_ident();
                    self.skip_ws();
                    let (op, value) = self.parse_attr_op();
                    self.skip_ws();
                    if self.peek() == Some(b']') {
                        self.pos += 1;
                    }
                    out.push(SimpleSelector::Attribute {
                        name: SmolStr::new(name.to_ascii_lowercase()),
                        op,
                        value: value.map(SmolStr::new),
                    });
                }
                Some(b':') => {
                    self.pos += 1;
                    let is_element = if self.peek() == Some(b':') {
                        self.pos += 1;
                        true
                    } else {
                        false
                    };
                    let name = self.consume_ident();
                    // Optional argument
                    if self.peek() == Some(b'(') {
                        self.pos += 1;
                        let mut depth = 1;
                        let start = self.pos;
                        while depth > 0 && self.pos < self.src.len() {
                            match self.src[self.pos] {
                                b'(' => depth += 1,
                                b')' => depth -= 1,
                                _ => {}
                            }
                            if depth > 0 {
                                self.pos += 1;
                            }
                        }
                        let arg = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("");
                        if self.peek() == Some(b')') {
                            self.pos += 1;
                        }
                        if is_element {
                            out.push(SimpleSelector::PseudoElement(SmolStr::new(name)));
                        } else {
                            let pc = match name.as_str() {
                                "not" => {
                                    if let Some(inner) = Selector::parse(arg) {
                                        PseudoClass::Not(Box::new(inner))
                                    } else {
                                        PseudoClass::Other(SmolStr::new(name))
                                    }
                                }
                                "nth-child" => {
                                    let arg = arg.trim();
                                    if arg == "n" || arg.is_empty() {
                                        PseudoClass::NthChild(None)
                                    } else {
                                        let n = arg.parse::<i32>().ok();
                                        PseudoClass::NthChild(n)
                                    }
                                }
                                _ => PseudoClass::Other(SmolStr::new(name)),
                            };
                            out.push(SimpleSelector::PseudoClass(pc));
                        }
                    } else if is_element {
                        out.push(SimpleSelector::PseudoElement(SmolStr::new(name)));
                    } else {
                        let pc = match name.as_str() {
                            "hover" => PseudoClass::Hover,
                            "focus" => PseudoClass::Focus,
                            "active" => PseudoClass::Active,
                            "visited" => PseudoClass::Visited,
                            "link" => PseudoClass::Link,
                            "first-child" => PseudoClass::FirstChild,
                            "last-child" => PseudoClass::LastChild,
                            "only-child" => PseudoClass::OnlyChild,
                            "empty" => PseudoClass::Empty,
                            "checked" => PseudoClass::Checked,
                            "disabled" => PseudoClass::Disabled,
                            "enabled" => PseudoClass::Enabled,
                            "root" => PseudoClass::Root,
                            _ => PseudoClass::Other(SmolStr::new(name)),
                        };
                        out.push(SimpleSelector::PseudoClass(pc));
                    }
                }
                _ => break,
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    fn parse_attr_op(&mut self) -> (AttrOp, Option<String>) {
        let op = match self.peek() {
            Some(b'=') => {
                self.pos += 1;
                AttrOp::Equals
            }
            Some(b'~') => {
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                }
                AttrOp::Includes
            }
            Some(b'|') => {
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                }
                AttrOp::DashMatch
            }
            Some(b'^') => {
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                }
                AttrOp::PrefixMatch
            }
            Some(b'$') => {
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                }
                AttrOp::SuffixMatch
            }
            Some(b'*') => {
                self.pos += 1;
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                }
                AttrOp::SubstringMatch
            }
            _ => AttrOp::Present,
        };
        if op == AttrOp::Present {
            return (op, None);
        }
        self.skip_ws();
        // Value: string or ident
        let v = match self.peek() {
            Some(b'"') | Some(b'\'') => {
                let q = self.src[self.pos];
                self.pos += 1;
                let start = self.pos;
                while self.pos < self.src.len() && self.src[self.pos] != q {
                    self.pos += 1;
                }
                let v = std::str::from_utf8(&self.src[start..self.pos])
                    .unwrap_or("")
                    .to_string();
                if self.pos < self.src.len() {
                    self.pos += 1;
                }
                v
            }
            _ => self.consume_ident(),
        };
        (op, Some(v))
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
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
        std::str::from_utf8(&self.src[start..self.pos])
            .unwrap_or("")
            .to_string()
    }
}

fn compute_specificity(compound: &[SimpleSelector]) -> (u32, u32, u32) {
    let mut a = 0;
    let mut b = 0;
    let mut c = 0;
    for s in compound {
        match s {
            SimpleSelector::Id(_) => a += 1,
            SimpleSelector::Class(_) | SimpleSelector::Attribute { .. } | SimpleSelector::PseudoClass(_) => {
                b += 1
            }
            SimpleSelector::Type(_) => c += 1,
            SimpleSelector::Universal | SimpleSelector::PseudoElement(_) => {}
        }
    }
    (a, b, c)
}
