//! Document Object Model — tree of nodes that represents a parsed HTML document.
//!
//! The DOM is the central data structure of the browser. The HTML parser produces
//! it, the CSS engine queries it, the layout engine walks it, the renderer paints
//! it, and the JavaScript engine reads and mutates it through the DOM API bridge.

use ahash::AHashMap;
use parking_lot::RwLock;
use smol_str::SmolStr;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

/// Globally-unique identifier for a node. We use a plain integer so that JS code
/// can hold references to nodes by id even when the underlying `Arc<Node>` is
/// dropped and re-created (e.g. after a parser rebuild).
static NEXT_NODE_ID: AtomicUsize = AtomicUsize::new(1);

pub fn next_node_id() -> usize {
    NEXT_NODE_ID.fetch_add(1, Ordering::Relaxed)
}

/// The kind of a DOM node. The split between `Element`, `Text`, `Comment` and
/// `Document` mirrors the DOM Living Standard.
#[derive(Debug, Clone)]
pub enum NodeKind {
    Document,
    Element(ElementData),
    Text(SmolStr),
    Comment(SmolStr),
    Doctype(DoctypeData),
}

#[derive(Debug, Clone, Default)]
pub struct DoctypeData {
    pub name: SmolStr,
    pub public_id: SmolStr,
    pub system_id: SmolStr,
}

#[derive(Debug, Clone)]
pub struct ElementData {
    pub tag: SmolStr,
    pub namespace: Namespace,
    pub attributes: AHashMap<SmolStr, SmolStr>,
    /// Insertion-ordered list of attribute names so we can iterate in document
    /// order (matters for `attributes`, `classList`, etc.).
    pub attr_order: Vec<SmolStr>,
}

impl ElementData {
    pub fn new(tag: impl Into<SmolStr>) -> Self {
        Self {
            tag: tag.into(),
            namespace: Namespace::Html,
            attributes: AHashMap::new(),
            attr_order: Vec::new(),
        }
    }

    pub fn get_attr(&self, name: &str) -> Option<&SmolStr> {
        self.attributes.get(name)
    }

    pub fn set_attr(&mut self, name: impl Into<SmolStr>, value: impl Into<SmolStr>) {
        let name: SmolStr = name.into();
        if !self.attributes.contains_key(&name) {
            self.attr_order.push(name.clone());
        }
        self.attributes.insert(name, value.into());
    }

    pub fn remove_attr(&mut self, name: &str) {
        if self.attributes.remove(name).is_some() {
            self.attr_order.retain(|n| n != name);
        }
    }

    pub fn id(&self) -> Option<&SmolStr> {
        self.attributes.get("id")
    }

    pub fn classes(&self) -> Vec<SmolStr> {
        match self.attributes.get("class") {
            Some(c) => c.split_whitespace().map(SmolStr::new).collect(),
            None => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Namespace {
    #[default]
    Html,
    Svg,
    MathMl,
}

/// A node in the DOM tree. Nodes are reference-counted and have a single parent
/// (or none, for the document). Children are stored as strong references in a
/// `RefCell` so they can be mutated by the parser and by JS.
///
/// We use `Arc<RwLock<NodeInner>>` rather than `Rc<RefCell>` so that the tree
/// can be shared between the layout thread and the JS thread safely.
pub type NodeRef = Arc<Node>;

pub struct Node {
    pub id: usize,
    inner: RwLock<NodeInner>,
}

struct NodeInner {
    kind: NodeKind,
    parent: Weak<Node>,
    children: Vec<NodeRef>,
    /// Per-node style computed by the CSS cascade. Stored here so layout and
    /// render can read it without re-running the cascade.
    style: ComputedStyle,
    /// Layout box produced by the layout engine. Cached here so hit-testing
    /// and JS APIs like `getBoundingClientRect` can use it.
    layout: Option<LayoutRect>,
    /// Event listeners registered from JS keyed by event type.
    listeners: AHashMap<String, Vec<EventListener>>,
    /// Value of an `<input>` / `<textarea>` as modified by the user. The DOM
    /// attribute reflects the initial value; this is the current "dirty" value.
    input_value: Option<String>,
    /// Whether a checkbox / radio is currently checked.
    checked: bool,
}

#[derive(Clone)]
pub struct EventListener {
    /// Identifies a JS handler registered with the runtime. 0 means "no
    /// handler" (used for inline `onclick="..."` attributes which are stored
    /// as raw source on the element itself).
    pub handler_id: usize,
    pub capture: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ComputedStyle {
    pub display: Display,
    pub position: Position,
    pub color: Option<u32>,
    pub background_color: Option<u32>,
    pub font_size: Option<f32>,
    pub font_weight: Option<u16>,
    pub font_style: Option<FontStyle>,
    pub font_family: Option<SmolStr>,
    pub margin: EdgeSizes,
    pub margin_auto_left: bool,
    pub margin_auto_right: bool,
    pub padding: EdgeSizes,
    pub border: EdgeSizes,
    pub border_color: Option<u32>,
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub min_width: Option<Length>,
    pub max_width: Option<Length>,
    pub flex_direction: Option<FlexDirection>,
    pub justify_content: Option<JustifyContent>,
    pub align_items: Option<AlignItems>,
    pub flex_wrap: Option<FlexWrap>,
    pub flex_grow: Option<f32>,
    pub flex_shrink: Option<f32>,
    pub flex_basis: Option<Length>,
    pub grid_template_columns: Vec<TrackSize>,
    pub grid_template_rows: Vec<TrackSize>,
    pub grid_column: Option<(i32, i32)>,
    pub grid_row: Option<(i32, i32)>,
    pub top: Option<Length>,
    pub right: Option<Length>,
    pub bottom: Option<Length>,
    pub left: Option<Length>,
    pub z_index: Option<i32>,
    pub overflow: Option<Overflow>,
    pub text_align: Option<TextAlign>,
    pub line_height: Option<f32>,
    pub visibility: Option<Visibility>,
    pub opacity: Option<f32>,
    pub box_sizing: BoxSizing,
    pub raw: AHashMap<SmolStr, SmolStr>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BoxSizing {
    #[default]
    ContentBox,
    BorderBox,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Display {
    #[default]
    Inline,
    Block,
    InlineBlock,
    Flex,
    InlineFlex,
    Grid,
    InlineGrid,
    None,
    ListItem,
    Table,
    TableRow,
    TableCell,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlexDirection {
    #[default]
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum JustifyContent {
    #[default]
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AlignItems {
    #[default]
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlexWrap {
    #[default]
    NoWrap,
    Wrap,
    WrapReverse,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Overflow {
    #[default]
    Visible,
    Hidden,
    Scroll,
    Auto,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
    Collapse,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum Length {
    #[default]
    Auto,
    Px(f32),
    Percent(f32),
    Em(f32),
    Rem(f32),
    Vw(f32),
    Vh(f32),
    Fr(f32),
}

#[derive(Debug, Clone, Copy, Default)]
pub enum TrackSize {
    #[default]
    Auto,
    Px(f32),
    Percent(f32),
    Fr(f32),
    MinContent,
    MaxContent,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EdgeSizes {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Node {
    pub fn new(kind: NodeKind) -> NodeRef {
        Arc::new(Node {
            id: next_node_id(),
            inner: RwLock::new(NodeInner {
                kind,
                parent: Weak::new(),
                children: Vec::new(),
                style: ComputedStyle::default(),
                layout: None,
                listeners: AHashMap::new(),
                input_value: None,
                checked: false,
            }),
        })
    }

    pub fn new_document() -> NodeRef {
        Self::new(NodeKind::Document)
    }

    pub fn new_element(tag: impl Into<SmolStr>) -> NodeRef {
        Self::new(NodeKind::Element(ElementData::new(tag)))
    }

    pub fn new_text(text: impl Into<SmolStr>) -> NodeRef {
        Self::new(NodeKind::Text(text.into()))
    }

    pub fn id(&self) -> usize {
        self.id
    }

    pub fn kind(&self) -> NodeKind {
        self.inner.read().kind.clone()
    }

    pub fn is_element(&self) -> bool {
        matches!(self.inner.read().kind, NodeKind::Element(_))
    }

    pub fn is_text(&self) -> bool {
        matches!(self.inner.read().kind, NodeKind::Text(_))
    }

    pub fn tag_name(&self) -> Option<SmolStr> {
        match &self.inner.read().kind {
            NodeKind::Element(e) => Some(e.tag.clone()),
            _ => None,
        }
    }

    pub fn text_content(&self) -> String {
        match &self.inner.read().kind {
            NodeKind::Text(t) => t.to_string(),
            NodeKind::Comment(_) => String::new(),
            NodeKind::Element(_) => {
                let mut out = String::new();
                for child in self.children() {
                    out.push_str(&child.text_content());
                }
                out
            }
            _ => String::new(),
        }
    }

    pub fn set_text(&self, text: impl Into<SmolStr>) {
        let mut g = self.inner.write();
        if let NodeKind::Text(t) = &mut g.kind {
            *t = text.into();
        }
    }

    pub fn parent(&self) -> Option<NodeRef> {
        self.inner.read().parent.upgrade()
    }

    pub fn children(&self) -> Vec<NodeRef> {
        self.inner.read().children.clone()
    }

    pub fn first_child(&self) -> Option<NodeRef> {
        self.inner.read().children.first().cloned()
    }

    pub fn last_child(&self) -> Option<NodeRef> {
        self.inner.read().children.last().cloned()
    }

    pub fn next_sibling(&self) -> Option<NodeRef> {
        let parent = self.parent()?;
        let children = parent.children();
        let idx = children.iter().position(|c| c.id == self.id)?;
        children.get(idx + 1).cloned()
    }

    pub fn previous_sibling(&self) -> Option<NodeRef> {
        let parent = self.parent()?;
        let children = parent.children();
        let idx = children.iter().position(|c| c.id == self.id)?;
        if idx == 0 {
            None
        } else {
            children.get(idx - 1).cloned()
        }
    }

    pub fn append_child(self_ref: &NodeRef, child: NodeRef) {
        // Detach from old parent first.
        if let Some(old) = child.parent() {
            Node::remove_child(&old, &child);
        }
        let mut g = self_ref.inner.write();
        {
            let mut cg = child.inner.write();
            cg.parent = Arc::downgrade(self_ref);
        }
        g.children.push(child);
    }

    pub fn insert_before(self_ref: &NodeRef, new_node: NodeRef, ref_node: Option<&NodeRef>) {
        let mut g = self_ref.inner.write();
        {
            let mut cg = new_node.inner.write();
            cg.parent = Arc::downgrade(self_ref);
        }
        match ref_node {
            None => g.children.push(new_node),
            Some(r) => {
                let idx = g.children.iter().position(|c| c.id == r.id);
                match idx {
                    Some(i) => g.children.insert(i, new_node),
                    None => g.children.push(new_node),
                }
            }
        }
    }

    pub fn remove_child(self_ref: &NodeRef, child: &NodeRef) -> Option<NodeRef> {
        let mut g = self_ref.inner.write();
        let idx = g.children.iter().position(|c| c.id == child.id)?;
        let removed = g.children.remove(idx);
        {
            let mut cg = removed.inner.write();
            cg.parent = Weak::new();
        }
        Some(removed)
    }

    pub fn replace_child(self_ref: &NodeRef, new_child: NodeRef, old_child: &NodeRef) -> Option<NodeRef> {
        let mut g = self_ref.inner.write();
        let idx = g.children.iter().position(|c| c.id == old_child.id)?;
        let old = std::mem::replace(&mut g.children[idx], new_child.clone());
        {
            let mut cg = old.inner.write();
            cg.parent = Weak::new();
        }
        {
            let mut cg = new_child.inner.write();
            cg.parent = Arc::downgrade(self_ref);
        }
        Some(old)
    }

    pub fn clear_children(&self) {
        let mut g = self.inner.write();
        for c in g.children.drain(..) {
            let mut cg = c.inner.write();
            cg.parent = Weak::new();
        }
    }

    /// Update the kind in place (used by the parser to fix up nodes after
    /// construction, and by JS to support `textContent` on elements).
    pub fn replace_kind(&self, kind: NodeKind) {
        self.inner.write().kind = kind;
    }

    /// Set an attribute on this element. No-op for non-element nodes.
    pub fn set_attribute(&self, name: &str, value: &str) {
        let mut g = self.inner.write();
        if let NodeKind::Element(ref mut e) = g.kind {
            e.set_attr(name, value);
        }
    }

    /// Get an attribute on this element. Returns `None` for non-element nodes
    /// or missing attributes.
    pub fn get_attribute(&self, name: &str) -> Option<String> {
        let g = self.inner.read();
        if let NodeKind::Element(ref e) = g.kind {
            e.get_attr(name).map(|s| s.to_string())
        } else {
            None
        }
    }

    /// Remove an attribute.
    pub fn remove_attribute(&self, name: &str) {
        let mut g = self.inner.write();
        if let NodeKind::Element(ref mut e) = g.kind {
            e.remove_attr(name);
        }
    }

    pub fn style(&self) -> ComputedStyle {
        self.inner.read().style.clone()
    }

    pub fn set_style(&self, style: ComputedStyle) {
        self.inner.write().style = style;
    }

    pub fn layout(&self) -> Option<LayoutRect> {
        self.inner.read().layout
    }

    pub fn set_layout(&self, layout: LayoutRect) {
        self.inner.write().layout = Some(layout);
    }

    pub fn add_event_listener(&self, event_type: &str, handler_id: usize, capture: bool) {
        let mut g = self.inner.write();
        g.listeners
            .entry(event_type.to_string())
            .or_default()
            .push(EventListener { handler_id, capture });
    }

    pub fn remove_event_listeners(&self, event_type: &str) {
        self.inner.write().listeners.remove(event_type);
    }

    pub fn event_listeners(&self, event_type: &str) -> Vec<EventListener> {
        self.inner
            .read()
            .listeners
            .get(event_type)
            .cloned()
            .unwrap_or_default()
    }

    pub fn has_event_listeners(&self, event_type: &str) -> bool {
        self.inner
            .read()
            .listeners
            .get(event_type)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    pub fn input_value(&self) -> Option<String> {
        self.inner.read().input_value.clone()
    }

    pub fn set_input_value(&self, value: Option<String>) {
        self.inner.write().input_value = value;
    }

    pub fn checked(&self) -> bool {
        self.inner.read().checked
    }

    pub fn set_checked(&self, checked: bool) {
        self.inner.write().checked = checked;
    }

    /// Pre-order walk starting at `root`. Calls `f` on the root first; if `f`
    /// returns false, that subtree is skipped.
    pub fn walk_ref(root: &NodeRef, f: &mut impl FnMut(&NodeRef) -> bool) {
        let mut stack: Vec<NodeRef> = vec![root.clone()];
        while let Some(n) = stack.pop() {
            if !f(&n) {
                continue;
            }
            let children = n.children();
            for c in children.into_iter().rev() {
                stack.push(c);
            }
        }
    }

    pub fn descendants(&self) -> Vec<NodeRef> {
        let mut out = Vec::new();
        for c in self.children() {
            Node::walk_ref(&c, &mut |n| {
                out.push(n.clone());
                true
            });
        }
        out
    }

    pub fn get_element_by_id(self_ref: &NodeRef, id: &str) -> Option<NodeRef> {
        let mut found = None;
        Node::walk_ref(self_ref, &mut |n| {
            if let NodeKind::Element(e) = &n.inner.read().kind {
                if e.get_attr("id").map(|s| s.as_str()) == Some(id) {
                    found = Some(n.clone());
                    return false;
                }
            }
            true
        });
        found
    }

    pub fn get_elements_by_tag_name(self_ref: &NodeRef, tag: &str) -> Vec<NodeRef> {
        let tag = tag.to_lowercase();
        let mut out = Vec::new();
        Node::walk_ref(self_ref, &mut |n| {
            if let NodeKind::Element(e) = &n.inner.read().kind {
                if e.tag.to_lowercase() == tag {
                    out.push(n.clone());
                }
            }
            true
        });
        out
    }

    pub fn get_elements_by_class(self_ref: &NodeRef, class: &str) -> Vec<NodeRef> {
        let mut out = Vec::new();
        Node::walk_ref(self_ref, &mut |n| {
            if let NodeKind::Element(e) = &n.inner.read().kind {
                if e.classes().iter().any(|c| c == class) {
                    out.push(n.clone());
                }
            }
            true
        });
        out
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = self.inner.read().kind.clone();
        f.debug_struct("Node")
            .field("id", &self.id)
            .field("kind", &kind)
            .finish()
    }
}

/// Convenience helpers used by the parser to construct common nodes.
pub fn document() -> NodeRef {
    Node::new_document()
}

pub fn element(tag: &str) -> NodeRef {
    Node::new_element(tag.to_lowercase())
}

pub fn text(s: &str) -> NodeRef {
    Node::new_text(s)
}

/// HTML void elements — they never have children and don't need a closing tag.
pub fn is_void_element(tag: &str) -> bool {
    matches!(
        tag.to_ascii_lowercase().as_str(),
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta"
            | "param" | "source" | "track" | "wbr"
    )
}

/// Elements whose content is raw text (not parsed as HTML).
pub fn is_raw_text_element(tag: &str) -> bool {
    matches!(
        tag.to_ascii_lowercase().as_str(),
        "script" | "style" | "textarea" | "title"
    )
}

/// Special elements that auto-close when certain other elements appear.
/// Mirrors a subset of the HTML5 adoption agency rules.
pub fn auto_closes(open: &str, new: &str) -> bool {
    let open = open.to_ascii_lowercase();
    let new = new.to_ascii_lowercase();
    if open == "li" && new == "li" {
        return true;
    }
    if (open == "td" || open == "th") && (new == "td" || new == "th" || new == "tr") {
        return true;
    }
    if open == "tr" && (new == "tr" || new == "tbody" || new == "thead" || new == "tfoot") {
        return true;
    }
    if (open == "thead" || open == "tbody" || open == "tfoot")
        && (new == "thead" || new == "tbody" || new == "tfoot")
    {
        return true;
    }
    if open == "option" && (new == "option" || new == "optgroup") {
        return true;
    }
    if open == "optgroup" && new == "optgroup" {
        return true;
    }
    if open == "p" {
        let closer = |c: &str| matches!(
            c,
            "address" | "article" | "aside" | "blockquote" | "div" | "dl" | "fieldset" | "footer"
                | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "header" | "hr" | "main" | "menu" | "nav"
                | "ol" | "p" | "pre" | "section" | "table" | "ul"
        );
        if closer(&new) {
            return true;
        }
    }
    false
}

/// Default display for a tag, used when no CSS rule sets `display`.
pub fn default_display(tag: &str) -> Display {
    match tag.to_ascii_lowercase().as_str() {
        "html" | "body" | "div" | "section" | "article" | "header" | "footer" | "main" | "nav"
        | "aside" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "li" | "table"
        | "thead" | "tbody" | "tfoot" | "tr" | "form" | "fieldset" | "blockquote" | "pre"
        | "figure" | "figcaption" | "details" | "summary" | "address" | "hr" | "dl" | "dt"
        | "dd" | "center" => Display::Block,
        "head" | "meta" | "link" | "title" | "base" | "style" | "script" => Display::None,
        "img" | "input" => Display::InlineBlock,
        "br" | "span" | "a" | "em" | "strong" | "b" | "i" | "u" | "code"
        | "small" | "sub" | "sup" | "label" | "abbr" | "cite" | "q" | "mark" | "time" | "var"
        | "kbd" | "samp" => Display::Inline,
        _ => Display::Inline,
    }
}
