//! DOM API bridge: exposes `document`, `Element`, `Node`, etc. to JS.
//!
//! Strategy: we install a substantial JS shim that defines `document`,
//! `Element`, `Node`, etc. as JS objects backed by Rust helper functions.
//! The Rust helpers (`__dom_*`) take node ids as their first argument and
//! call into the DOM tree.

use super::runtime::{current_runtime, JsRuntime};
use boa_engine::object::builtins::JsArray;
use boa_engine::object::{FunctionObjectBuilder, JsObject};
use boa_engine::property::Attribute;
use boa_engine::{js_string, Context, JsArgs, JsResult, JsValue, NativeFunction, Source};
use smol_str::SmolStr;

use crate::html::dom::*;

fn make_fn(ctx: &mut Context, name: &str, native: NativeFunction) -> JsObject {
    let f: JsObject = FunctionObjectBuilder::new(ctx.realm(), native)
        .name(js_string!(name))
        .build()
        .into();
    f
}

fn s2v(s: &str) -> JsValue {
    JsValue::from(js_string!(s))
}

fn make_array(ctx: &mut Context, ids: &[usize]) -> JsValue {
    let arr = JsArray::new(ctx).expect("JsArray::new");
    for (i, cid) in ids.iter().enumerate() {
        let code = format!("__makeNode({})", cid);
        if let Ok(v) = ctx.eval(Source::from_bytes(code.as_bytes())) {
            let _ = arr.push(v, ctx);
        }
    }
    arr.into()
}

/// Install the DOM API into the runtime's context. Must be called once per
/// runtime, after globals are installed.
pub fn install_dom(rt: &JsRuntime) {
    rt.with_context(|ctx| {
        register_native_helpers(ctx);
        let shim = r#"
            (function() {
                function makeNode(nodeId) {
                    var el = {
                        __nodeId: nodeId,
                        __handlers: {},
                    };
                    Object.defineProperty(el, 'textContent', {
                        get: function() { return __node_textContent(this.__nodeId); },
                        set: function(v) { __node_setTextContent(this.__nodeId, String(v)); }
                    });
                    Object.defineProperty(el, 'innerHTML', {
                        get: function() { return __node_innerHTML(this.__nodeId); },
                        set: function(v) { __node_setInnerHTML(this.__nodeId, String(v)); }
                    });
                    Object.defineProperty(el, 'outerHTML', {
                        get: function() { return __node_outerHTML(this.__nodeId); },
                        set: function(v) { __node_setOuterHTML(this.__nodeId, String(v)); }
                    });
                    Object.defineProperty(el, 'id', {
                        get: function() { return __node_getAttribute(this.__nodeId, 'id') || ''; },
                        set: function(v) { __node_setAttribute(this.__nodeId, 'id', String(v)); }
                    });
                    Object.defineProperty(el, 'className', {
                        get: function() { return __node_getAttribute(this.__nodeId, 'class') || ''; },
                        set: function(v) { __node_setAttribute(this.__nodeId, 'class', String(v)); }
                    });
                    Object.defineProperty(el, 'tagName', {
                        get: function() { return __node_tagName(this.__nodeId); }
                    });
                    Object.defineProperty(el, 'nodeName', {
                        get: function() { return __node_tagName(this.__nodeId); }
                    });
                    Object.defineProperty(el, 'nodeType', {
                        get: function() { return __node_nodeType(this.__nodeId); }
                    });
                    Object.defineProperty(el, 'value', {
                        get: function() { return __node_value(this.__nodeId); },
                        set: function(v) { __node_setValue(this.__nodeId, String(v)); }
                    });
                    Object.defineProperty(el, 'checked', {
                        get: function() { return __node_checked(this.__nodeId); },
                        set: function(v) { __node_setChecked(this.__nodeId, !!v); }
                    });
                    Object.defineProperty(el, 'style', {
                        get: function() {
                            var s = __node_style(this.__nodeId);
                            var obj = {};
                            var parts = s.split(';');
                            for (var i = 0; i < parts.length; i++) {
                                var p = parts[i].trim();
                                if (!p) continue;
                                var idx = p.indexOf(':');
                                if (idx < 0) continue;
                                var k = p.slice(0, idx).trim();
                                var v = p.slice(idx + 1).trim();
                                obj[k] = v;
                            }
                            var self = this;
                            return new Proxy(obj, {
                                set: function(target, prop, value) {
                                    target[prop] = value;
                                    var out = [];
                                    for (var k in target) out.push(k + ': ' + target[k]);
                                    __node_setStyle(self.__nodeId, out.join('; '));
                                    return true;
                                }
                            });
                        }
                    });
                    Object.defineProperty(el, 'parentNode', {
                        get: function() { return __node_parent(this.__nodeId); }
                    });
                    Object.defineProperty(el, 'parentElement', {
                        get: function() { return __node_parent(this.__nodeId); }
                    });
                    Object.defineProperty(el, 'children', {
                        get: function() { return __node_children(this.__nodeId); }
                    });
                    Object.defineProperty(el, 'childNodes', {
                        get: function() { return __node_children(this.__nodeId); }
                    });
                    Object.defineProperty(el, 'firstChild', {
                        get: function() { return __node_firstChild(this.__nodeId); }
                    });
                    Object.defineProperty(el, 'lastChild', {
                        get: function() { return __node_lastChild(this.__nodeId); }
                    });
                    Object.defineProperty(el, 'nextSibling', {
                        get: function() { return __node_nextSibling(this.__nodeId); }
                    });
                    Object.defineProperty(el, 'previousSibling', {
                        get: function() { return __node_prevSibling(this.__nodeId); }
                    });
                    Object.defineProperty(el, 'classList', {
                        get: function() {
                            var self = this;
                            var cs = (__node_getAttribute(this.__nodeId, 'class') || '').split(/\s+/).filter(Boolean);
                            return {
                                add: function(c) { if (cs.indexOf(c) < 0) cs.push(c); __node_setAttribute(self.__nodeId, 'class', cs.join(' ')); },
                                remove: function(c) { cs = cs.filter(function(x) { return x !== c; }); __node_setAttribute(self.__nodeId, 'class', cs.join(' ')); },
                                toggle: function(c) { if (cs.indexOf(c) < 0) cs.push(c); else cs = cs.filter(function(x) { return x !== c; }); __node_setAttribute(self.__nodeId, 'class', cs.join(' ')); },
                                contains: function(c) { return cs.indexOf(c) >= 0; },
                                length: cs.length,
                                item: function(i) { return cs[i] || null; },
                                toString: function() { return cs.join(' '); }
                            };
                        }
                    });
                    Object.defineProperty(el, 'attributes', {
                        get: function() { return __node_attributes(this.__nodeId); }
                    });
                    el.getAttribute = function(n) { var v = __node_getAttribute(this.__nodeId, n); return v === null ? null : v; };
                    el.setAttribute = function(n, v) { __node_setAttribute(this.__nodeId, n, String(v)); };
                    el.removeAttribute = function(n) { __node_removeAttribute(this.__nodeId, n); };
                    el.hasAttribute = function(n) { return __node_getAttribute(this.__nodeId, n) !== null; };
                    el.appendChild = function(c) { if (c && typeof c.__nodeId === 'number') __node_appendChild(this.__nodeId, c.__nodeId); return c; };
                    el.removeChild = function(c) { if (c && typeof c.__nodeId === 'number') __node_removeChild(this.__nodeId, c.__nodeId); return c; };
                    el.insertBefore = function(c, r) { __node_insertBefore(this.__nodeId, c.__nodeId, r ? r.__nodeId : 0); return c; };
                    el.replaceChild = function(c, o) { __node_replaceChild(this.__nodeId, c.__nodeId, o.__nodeId); return o; };
                    el.cloneNode = function(deep) { return __node_clone(this.__nodeId, !!deep); };
                    el.contains = function(c) { return __node_contains(this.__nodeId, c.__nodeId); };
                    el.addEventListener = function(type, handler, opts) {
                        var id = __node_addEventListener(this.__nodeId, type, handler);
                        if (!this.__handlers[type]) this.__handlers[type] = [];
                        this.__handlers[type].push(id);
                        return id;
                    };
                    el.removeEventListener = function(type, handler) {
                        __node_removeEventListener(this.__nodeId, type);
                    };
                    el.dispatchEvent = function(type) {
                        return __node_dispatchEvent(this.__nodeId, typeof type === 'string' ? type : (type && type.type) || '');
                    };
                    el.querySelector = function(sel) { return nullToStub(__node_querySelector(this.__nodeId, sel)); };
                    el.querySelectorAll = function(sel) { return __node_querySelectorAll(this.__nodeId, sel); };
                    el.getElementById = function(id) { return nullToStub(__node_getElementById(this.__nodeId, id)); };
                    el.getElementsByTagName = function(t) { return __node_getElementsByTagName(this.__nodeId, t); };
                    el.getElementsByClassName = function(c) { return __node_getElementsByClassName(this.__nodeId, c); };
                    el.getBoundingClientRect = function() { return __node_getBoundingClientRect(this.__nodeId); };
                    el.click = function() { __node_dispatchEvent(this.__nodeId, 'click'); };
                    el.focus = function() { __node_dispatchEvent(this.__nodeId, 'focus'); };
                    el.blur = function() { __node_dispatchEvent(this.__nodeId, 'blur'); };
                    el.submit = function() { __node_dispatchEvent(this.__nodeId, 'submit'); };
                    el.insertAdjacentHTML = function(pos, html) {
                        // Best-effort: parse html and insert relative to this node.
                        // Positions: 'beforebegin', 'afterbegin', 'beforeend', 'afterend'
                        __node_insertAdjacentHTML(this.__nodeId, String(pos), String(html));
                    };
                    el.insertAdjacentText = function(pos, text) {
                        // Treat as inserting a text node.
                        __node_insertAdjacentHTML(this.__nodeId, String(pos), String(text));
                    };
                    el.remove = function() { __node_remove(this.__nodeId); };
                    el.prepend = function() {
                        // Insert all arguments at the beginning of this node's children.
                        for (var i = 0; i < arguments.length; i++) {
                            var arg = arguments[i];
                            if (arg && typeof arg.__nodeId === 'number') {
                                __node_insertBefore(this.__nodeId, arg.__nodeId, this.firstChild ? this.firstChild.__nodeId : 0);
                            } else {
                                var tn = __document_createTextNode(String(arg));
                                __node_insertBefore(this.__nodeId, tn, this.firstChild ? this.firstChild.__nodeId : 0);
                            }
                        }
                    };
                    el.append = function() {
                        for (var i = 0; i < arguments.length; i++) {
                            var arg = arguments[i];
                            if (arg && typeof arg.__nodeId === 'number') {
                                __node_appendChild(this.__nodeId, arg.__nodeId);
                            } else {
                                var tn = __document_createTextNode(String(arg));
                                __node_appendChild(this.__nodeId, tn);
                            }
                        }
                    };
                    el.before = function() {
                        var parent = this.parentNode;
                        if (!parent) return;
                        for (var i = 0; i < arguments.length; i++) {
                            var arg = arguments[i];
                            if (arg && typeof arg.__nodeId === 'number') {
                                __node_insertBefore(parent.__nodeId, arg.__nodeId, this.__nodeId);
                            }
                        }
                    };
                    el.after = function() {
                        var parent = this.parentNode;
                        if (!parent) return;
                        var ref = this.nextSibling ? this.nextSibling.__nodeId : 0;
                        for (var i = 0; i < arguments.length; i++) {
                            var arg = arguments[i];
                            if (arg && typeof arg.__nodeId === 'number') {
                                __node_insertBefore(parent.__nodeId, arg.__nodeId, ref);
                            }
                        }
                    };
                    el.addEventListener = function(type, handler, opts) {
                        var id = __node_addEventListener(this.__nodeId, type, handler);
                        if (!this.__handlers[type]) this.__handlers[type] = [];
                        this.__handlers[type].push(id);
                        return id;
                    };
                    el.removeEventListener = function(type, handler) {
                        __node_removeEventListener(this.__nodeId, type);
                    };
                    el.dispatchEvent = function(type) {
                        return __node_dispatchEvent(this.__nodeId, typeof type === 'string' ? type : (type && type.type) || '');
                    };
                    el.toString = function() { return '[object HTMLElement]'; };
                    return el;
                }
                globalThis.__makeNode = makeNode;
                // A "null node" stub — returned by getElementById etc. when no
                // match is found, so that scripts which chain method calls on
                // the result (e.g. document.getElementById('x').appendChild(...))
                // don't throw a TypeError. All methods are no-ops; all property
                // accesses return null/undefined/0 as appropriate.
                function makeNullNode() {
                    var el = {
                        __nodeId: -1,
                        __handlers: {},
                        __isNull: true,
                    };
                    Object.defineProperty(el, 'textContent', { get: function() { return ''; }, set: function() {} });
                    Object.defineProperty(el, 'innerHTML', { get: function() { return ''; }, set: function() {} });
                    Object.defineProperty(el, 'outerHTML', { get: function() { return ''; }, set: function() {} });
                    Object.defineProperty(el, 'id', { get: function() { return ''; }, set: function() {} });
                    Object.defineProperty(el, 'className', { get: function() { return ''; }, set: function() {} });
                    Object.defineProperty(el, 'tagName', { get: function() { return ''; } });
                    Object.defineProperty(el, 'nodeName', { get: function() { return ''; } });
                    Object.defineProperty(el, 'nodeType', { get: function() { return 0; } });
                    Object.defineProperty(el, 'value', { get: function() { return ''; }, set: function() {} });
                    Object.defineProperty(el, 'checked', { get: function() { return false; }, set: function() {} });
                    Object.defineProperty(el, 'style', { get: function() { return {}; } });
                    Object.defineProperty(el, 'parentNode', { get: function() { return null; } });
                    Object.defineProperty(el, 'parentElement', { get: function() { return null; } });
                    Object.defineProperty(el, 'children', { get: function() { return []; } });
                    Object.defineProperty(el, 'childNodes', { get: function() { return []; } });
                    Object.defineProperty(el, 'firstChild', { get: function() { return null; } });
                    Object.defineProperty(el, 'lastChild', { get: function() { return null; } });
                    Object.defineProperty(el, 'nextSibling', { get: function() { return null; } });
                    Object.defineProperty(el, 'previousSibling', { get: function() { return null; } });
                    Object.defineProperty(el, 'classList', { get: function() { return { add: function(){}, remove: function(){}, toggle: function(){}, contains: function(){ return false; }, length: 0, item: function(){ return null; } }; } });
                    Object.defineProperty(el, 'attributes', { get: function() { return []; } });
                    el.getAttribute = function() { return null; };
                    el.setAttribute = function() {};
                    el.removeAttribute = function() {};
                    el.hasAttribute = function() { return false; };
                    el.appendChild = function(c) { return c; };
                    el.removeChild = function(c) { return c; };
                    el.insertBefore = function(c) { return c; };
                    el.replaceChild = function(c, o) { return o; };
                    el.cloneNode = function() { return makeNullNode(); };
                    el.contains = function() { return false; };
                    el.addEventListener = function() {};
                    el.removeEventListener = function() {};
                    el.dispatchEvent = function() { return true; };
                    el.querySelector = function() { return null; };
                    el.querySelectorAll = function() { return []; };
                    el.getElementById = function() { return null; };
                    el.getElementsByTagName = function() { return []; };
                    el.getElementsByClassName = function() { return []; };
                    el.getBoundingClientRect = function() { return { x: 0, y: 0, left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0 }; };
                    el.click = function() {};
                    el.focus = function() {};
                    el.blur = function() {};
                    el.submit = function() {};
                    el.insertAdjacentHTML = function() {};
                    el.insertAdjacentText = function() {};
                    el.remove = function() {};
                    el.prepend = function() {};
                    el.append = function() {};
                    el.before = function() {};
                    el.after = function() {};
                    el.toString = function() { return '[object NullElement]'; };
                    return el;
                }
                globalThis.__makeNullNode = makeNullNode;
                function nullToStub(n) { return (n === null || n === undefined) ? makeNullNode() : n; }
                globalThis.document = {
                    __nodeId: 0,
                    createElement: function(tag) { return makeNode(__document_createElement(String(tag).toLowerCase())); },
                    createElementNS: function(ns, tag) { return makeNode(__document_createElement(String(tag).toLowerCase())); },
                    createTextNode: function(t) { return makeNode(__document_createTextNode(String(t))); },
                    createComment: function(t) { return makeNode(__document_createTextNode('')); },
                    createDocumentFragment: function() { return makeNode(__document_createElement('div')); },
                    getElementById: function(id) { return nullToStub(__node_getElementById(0, id)); },
                    querySelector: function(sel) { return nullToStub(__node_querySelector(0, sel)); },
                    querySelectorAll: function(sel) { return __node_querySelectorAll(0, sel); },
                    getElementsByTagName: function(t) { return __node_getElementsByTagName(0, t); },
                    getElementsByClassName: function(c) { return __node_getElementsByClassName(0, c); },
                    getElementsByName: function(n) { return []; },
                    addEventListener: function(type, handler) { return __node_addEventListener(0, type, handler); },
                    removeEventListener: function(type, handler) { __node_removeEventListener(0, type); },
                    dispatchEvent: function(type) { return __node_dispatchEvent(0, typeof type === 'string' ? type : (type && type.type) || ''); },
                    body: null,
                    head: null,
                    documentElement: null,
                    title: '',
                    cookie: '',
                    domain: '',
                    URL: __document_url(),
                    readyState: 'complete',
                    visibilityState: 'visible',
                    write: function(html) { __document_write(String(html)); },
                    writeln: function(html) { __document_write(String(html) + '\n'); },
                };
                __document_init();
                globalThis.Element = function(){};
                globalThis.Node = function(){};
                globalThis.HTMLElement = function(){};
                globalThis.Document = function(){};
                globalThis.DocumentFragment = function(){ return document.createDocumentFragment(); };
                globalThis.ShadowRoot = function(){};
                globalThis.HTMLDivElement = function(){};
                globalThis.HTMLSpanElement = function(){};
                globalThis.HTMLInputElement = function(){};
                globalThis.HTMLBodyElement = function(){};
                globalThis.HTMLHeadElement = function(){};
                globalThis.HTMLAnchorElement = function(){};
                globalThis.HTMLImageElement = function(){};
                globalThis.HTMLScriptElement = function(){};
                globalThis.HTMLLinkElement = function(){};
                globalThis.HTMLFormElement = function(){};
                globalThis.HTMLButtonElement = function(){};
                globalThis.HTMLCanvasElement = function(){};
                globalThis.Text = function(t) { return document.createTextNode(t); };
                globalThis.Comment = function(t) { return document.createComment(t); };
                globalThis.Event = function(type, opts) {
                    this.type = type;
                    this.bubbles = false;
                    this.cancelable = false;
                    this.target = null;
                    this.currentTarget = null;
                    this.preventDefault = function(){};
                    this.stopPropagation = function(){};
                };
                globalThis.CustomEvent = globalThis.Event;
                globalThis.MouseEvent = globalThis.Event;
                globalThis.KeyboardEvent = globalThis.Event;
                // navigator — minimal stub
                globalThis.navigator = {
                    userAgent: 'Z-Browser/0.1',
                    language: 'en-US',
                    languages: ['en-US', 'en'],
                    platform: 'Linux',
                    onLine: true,
                    cookieEnabled: false,
                };
                // location — read-only stub
                globalThis.location = {
                    href: __document_url(),
                    origin: __document_url(),
                    host: '',
                    hostname: '',
                    port: '',
                    pathname: '/',
                    search: '',
                    hash: '',
                    toString: function() { return this.href; },
                    assign: function(u) { if (typeof __browser_navigate === 'function') __browser_navigate(String(u)); },
                    replace: function(u) { if (typeof __browser_navigate === 'function') __browser_navigate(String(u)); },
                    reload: function() { if (typeof __browser_reload === 'function') __browser_reload(); },
                };
                // history stub
                globalThis.history = {
                    length: 1,
                    back: function() {},
                    forward: function() {},
                    go: function(n) {},
                    pushState: function(state, title, url) {},
                    replaceState: function(state, title, url) {},
                };
                // screen stub
                globalThis.screen = { width: 1280, height: 720, availWidth: 1280, availHeight: 720, colorDepth: 24 };
                // JSON — Boa should have this but ensure it exists
                if (typeof globalThis.JSON === 'undefined') {
                    globalThis.JSON = {
                        parse: function(s) { return eval('(' + s + ')'); },
                        stringify: function(v) {
                            if (v === null) return 'null';
                            if (typeof v === 'undefined') return undefined;
                            if (typeof v === 'string') return '"' + v.replace(/"/g, '\\"') + '"';
                            if (typeof v === 'number' || typeof v === 'boolean') return String(v);
                            if (Array.isArray(v)) { return '[' + v.map(JSON.stringify).join(',') + ']'; }
                            var parts = [];
                            for (var k in v) { if (v.hasOwnProperty(k)) { var s = JSON.stringify(v[k]); if (s !== undefined) parts.push('"' + k + '":' + s); } }
                            return '{' + parts.join(',') + '}';
                        }
                    };
                }
                // Performance stub
                globalThis.performance = { now: function() { return Date.now(); }, timing: {}, navigation: {} };
                // requestAnimationFrame / cancelAnimationFrame — call synchronously
                globalThis.requestAnimationFrame = function(cb) { try { cb(Date.now()); } catch(e) {} return 1; };
                globalThis.cancelAnimationFrame = function(id) {};
                // MutationObserver stub
                globalThis.MutationObserver = function(cb) { this.observe = function(){}; this.disconnect = function(){}; this.takeRecords = function(){ return []; }; };
                globalThis.IntersectionObserver = function(cb) { this.observe = function(){}; this.unobserve = function(){}; this.disconnect = function(){}; this.takeRecords = function(){ return []; }; };
                globalThis.ResizeObserver = function(cb) { this.observe = function(){}; this.unobserve = function(){}; this.disconnect = function(){}; };
                globalThis.PerformanceObserver = function(cb) { this.observe = function(){}; this.disconnect = function(){}; this.takeRecords = function(){ return []; }; };
                // TrustedTypePolicy / trustedTypes stub
                globalThis.trustedTypes = { createPolicy: function(name, rules) { return { createHTML: function(s){return s;}, createScript: function(s){return s;}, createScriptURL: function(s){return s;} }; }};
                globalThis.TrustedTypePolicy = function(){ this.createHTML = function(s){return s;}; this.createScript = function(s){return s;}; };
                globalThis.TrustedTypePolicyFactory = function(){};
                // CSS / style stubs
                globalThis.CSS = { supports: function() { return false; }, escape: function(s) { return s; } };
                globalThis.CSSStyleDeclaration = function(){};
                // URL constructor
                globalThis.URL = function(url, base) {
                    try {
                        var u = base ? new (globalThis.__NativeURL)(base).resolve(url) : url;
                        this.href = u;
                        this.origin = u.split('/').slice(0,3).join('/');
                        this.protocol = u.split(':')[0] + ':';
                        this.host = u.split('/')[2] || '';
                        this.hostname = (u.split('/')[2] || '').split(':')[0];
                        this.pathname = '/' + (u.split('/').slice(3).join('/').split('?')[0]);
                        this.search = (u.indexOf('?') >= 0) ? u.substring(u.indexOf('?')) : '';
                        this.hash = '';
                    } catch(e) { this.href = url; }
                    this.toString = function() { return this.href; };
                };
                globalThis.URLSearchParams = function(init) {
                    var params = {};
                    if (typeof init === 'string') {
                        init.split('&').forEach(function(p) {
                            var kv = p.split('=');
                            params[decodeURIComponent(kv[0])] = decodeURIComponent(kv[1] || '');
                        });
                    }
                    this.get = function(k) { return params[k] || null; };
                    this.set = function(k, v) { params[k] = v; };
                    this.has = function(k) { return k in params; };
                    this.append = function(k, v) { params[k] = v; };
                    this.delete = function(k) { delete params[k]; };
                    this.toString = function() {
                        return Object.keys(params).map(function(k){return k+'='+encodeURIComponent(params[k]);}).join('&');
                    };
                };
                // Symbol.iterator for arrays (Boa should have this but ensure)
                // TextEncoder/TextDecoder stubs
                globalThis.TextEncoder = function() { this.encode = function(s) { return new Uint8Array(s.split('').map(function(c){return c.charCodeAt(0);})); }; };
                globalThis.TextDecoder = function() { this.decode = function(b) { return String.fromCharCode.apply(null, b); }; };
                // AbortController stub
                globalThis.AbortController = function() { this.signal = { aborted: false, addEventListener: function(){}, removeEventListener: function(){} }; this.abort = function() { this.signal.aborted = true; }; };
                globalThis.AbortSignal = function() { this.aborted = false; this.addEventListener = function(){}; };
                // DOMParser stub
                globalThis.DOMParser = function() { this.parseFromString = function(str, type) { return document; }; };
                // XMLSerializer stub
                globalThis.XMLSerializer = function() { this.serializeToString = function(node) { return node.outerHTML || ''; }; };
                // customElements stub
                globalThis.customElements = { define: function(){}, get: function(){ return undefined; }, whenDefined: function(){ return Promise.resolve(); }, upgrade: function(){} };
                // Reflect stub (Boa should have this)
                if (typeof globalThis.Reflect === 'undefined') { globalThis.Reflect = { has: function(o,k){return k in o;}, get: function(o,k){return o[k];}, set: function(o,k,v){o[k]=v;return true;}, deleteProperty: function(o,k){delete o[k];return true;}, ownKeys: function(o){return Object.keys(o);} }; }
                // Promise.allSettled, Promise.any, Promise.race (Boa may have these)
                if (typeof Promise.allSettled === 'undefined') { Promise.allSettled = function(promises) { return Promise.all(promises.map(function(p){ return Promise.resolve(p).then(function(v){return {status:'fulfilled',value:v};}, function(e){return {status:'rejected',reason:e};}); })); }; }
                if (typeof Promise.any === 'undefined') { Promise.any = function(promises) { return new Promise(function(resolve, reject) { var errors = []; var pending = promises.length; if (pending === 0) { reject(new AggregateError([])); return; } promises.forEach(function(p) { Promise.resolve(p).then(resolve, function(e) { errors.push(e); if (--pending === 0) reject(errors[0] || new Error('All rejected')); }); }); }); }; }
                // AggregateError stub
                if (typeof globalThis.AggregateError === 'undefined') { globalThis.AggregateError = function(errors, msg) { this.errors = errors; this.message = msg; this.name = 'AggregateError'; }; globalThis.AggregateError.prototype = Object.create(Error.prototype); }
            })();
        "#;
        let _ = ctx.eval(Source::from_bytes(shim.as_bytes()));
    });
}

macro_rules! reg {
    ($ctx:expr, $name:expr, $arity:expr, $body:expr) => {{
        let _ = $ctx.register_global_callable(
            js_string!($name),
            $arity,
            NativeFunction::from_fn_ptr($body),
        );
    }};
}

fn register_native_helpers(ctx: &mut Context) {
    reg!(ctx, "__document_url", 0, |_this, _args, _ctx| {
        let rt = current_runtime();
        Ok(s2v(rt.base_url()))
    });
    reg!(ctx, "__document_createElement", 1, |_this, args, _ctx| {
        let tag = args
            .get_or_undefined(0)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let n = crate::html::dom::element(&tag);
        let id = n.id;
        current_runtime().register_node(&n);
        Ok(JsValue::new(id as f64))
    });
    reg!(ctx, "__document_createTextNode", 1, |_this, args, _ctx| {
        let text = args
            .get_or_undefined(0)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let n = crate::html::dom::text(&text);
        let id = n.id;
        current_runtime().register_node(&n);
        Ok(JsValue::new(id as f64))
    });
    reg!(ctx, "__document_init", 0, |_this, _args, ctx| {
        let rt = current_runtime();
        let doc = match rt.lookup_node(0) {
            Some(n) => n,
            None => return Ok(JsValue::undefined()),
        };
        let mut html_id = 0;
        let mut head_id = 0;
        let mut body_id = 0;
        for c in doc.children() {
            if c.tag_name().map(|t| t.eq_ignore_ascii_case("html")).unwrap_or(false) {
                html_id = c.id;
                for cc in c.children() {
                    if cc.tag_name().map(|t| t.eq_ignore_ascii_case("head")).unwrap_or(false) {
                        head_id = cc.id;
                    } else if cc.tag_name().map(|t| t.eq_ignore_ascii_case("body")).unwrap_or(false) {
                        body_id = cc.id;
                    }
                }
            }
        }
        let code = format!(
            "(function() {{
                if (window.__makeNode) {{
                    document.documentElement = __makeNode({});
                    document.head = __makeNode({});
                    document.body = __makeNode({});
                }}
            }})();",
            html_id, head_id, body_id
        );
        let _ = ctx.eval(Source::from_bytes(code.as_bytes()));
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__document_write", 1, |_this, args, _ctx| {
        // For simplicity, we ignore document.write after load. Just log it.
        if let Some(s) = args.get_or_undefined(0).as_string() {
            log::debug!("[document.write ignored] {}", s.to_std_string_escaped());
        }
        Ok(JsValue::undefined())
    });

    reg!(ctx, "__node_getAttribute", 2, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let name = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        match rt.lookup_node(id) {
            Some(n) => match n.get_attribute(&name) {
                Some(v) => Ok(s2v(&v)),
                None => Ok(JsValue::null()),
            },
            None => Ok(JsValue::null()),
        }
    });
    reg!(ctx, "__node_setAttribute", 3, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let name = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let value = args
            .get_or_undefined(2)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            n.set_attribute(&name, &value);
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_removeAttribute", 2, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let name = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            n.remove_attribute(&name);
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_textContent", 1, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        match rt.lookup_node(id) {
            Some(n) => Ok(s2v(&n.text_content())),
            None => Ok(JsValue::null()),
        }
    });
    reg!(ctx, "__node_setTextContent", 2, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let text = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            n.clear_children();
            let t = crate::html::dom::text(&text);
            rt.register_node(&t);
            Node::append_child(&n, t);
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_innerHTML", 1, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        match rt.lookup_node(id) {
            Some(n) => Ok(s2v(&serialize_subtree(&n))),
            None => Ok(JsValue::null()),
        }
    });
    reg!(ctx, "__node_setInnerHTML", 2, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let html = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            n.clear_children();
            let parsed = crate::html::parser::parse(&format!(
                "<html><body>{}</body></html>",
                html
            ));
            let body = find_body(&parsed);
            if let Some(b) = body {
                for c in b.children() {
                    rt.register_node(&c);
                    Node::append_child(&n, c);
                }
            }
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_outerHTML", 1, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        match rt.lookup_node(id) {
            Some(n) => Ok(s2v(&serialize_node(&n))),
            None => Ok(JsValue::null()),
        }
    });
    reg!(ctx, "__node_setOuterHTML", 2, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let html = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            if let Some(parent) = n.parent() {
                let parsed = crate::html::parser::parse(&format!(
                    "<html><body>{}</body></html>",
                    html
                ));
                let body = find_body(&parsed);
                if let Some(b) = body {
                    let children = b.children();
                    let insert_before = n.next_sibling();
                    for c in children {
                        rt.register_node(&c);
                        Node::insert_before(&parent, c.clone(), insert_before.as_ref());
                    }
                    Node::remove_child(&parent, &n);
                }
            }
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_tagName", 1, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        match rt.lookup_node(id) {
            Some(n) => Ok(s2v(&n.tag_name().map(|t| t.to_uppercase()).unwrap_or_default())),
            None => Ok(JsValue::undefined()),
        }
    });
    reg!(ctx, "__node_nodeType", 1, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        match rt.lookup_node(id) {
            Some(n) => {
                let kind = n.kind();
                let t = match kind {
                    NodeKind::Element(_) => 1,
                    NodeKind::Text(_) => 3,
                    NodeKind::Comment(_) => 8,
                    NodeKind::Document => 9,
                    NodeKind::Doctype(_) => 10,
                };
                Ok(JsValue::new(t))
            }
            None => Ok(JsValue::undefined()),
        }
    });
    reg!(ctx, "__node_value", 1, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            if let Some(v) = n.input_value() {
                return Ok(s2v(&v));
            }
            let kind = n.kind();
            if let NodeKind::Text(t) = kind {
                return Ok(s2v(&t.to_string()));
            }
            if let Some(v) = n.get_attribute("value") {
                return Ok(s2v(&v));
            }
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_setValue", 2, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let value = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            n.set_input_value(Some(value));
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_checked", 1, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            return Ok(JsValue::new(n.checked()));
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_setChecked", 2, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let checked = args.get_or_undefined(1).as_boolean().unwrap_or(false);
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            n.set_checked(checked);
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_style", 1, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            let s = n.get_attribute("style").unwrap_or_default();
            return Ok(s2v(&s));
        }
        Ok(s2v(""))
    });
    reg!(ctx, "__node_setStyle", 2, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let style = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            if style.is_empty() {
                n.remove_attribute("style");
            } else {
                n.set_attribute("style", &style);
            }
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_parent", 1, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            if let Some(p) = n.parent() {
                rt.register_node(&p);
                let code = format!("__makeNode({})", p.id);
                if let Ok(v) = ctx.eval(Source::from_bytes(code.as_bytes())) {
                    return Ok(v);
                }
            }
        }
        Ok(JsValue::null())
    });
    reg!(ctx, "__node_children", 1, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        let mut ids = Vec::new();
        if let Some(n) = rt.lookup_node(id) {
            for c in n.children() {
                rt.register_node(&c);
                ids.push(c.id);
            }
        }
        Ok(make_array(ctx, &ids))
    });
    reg!(ctx, "__node_firstChild", 1, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            if let Some(c) = n.first_child() {
                rt.register_node(&c);
                let code = format!("__makeNode({})", c.id);
                if let Ok(v) = ctx.eval(Source::from_bytes(code.as_bytes())) {
                    return Ok(v);
                }
            }
        }
        Ok(JsValue::null())
    });
    reg!(ctx, "__node_lastChild", 1, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            if let Some(c) = n.last_child() {
                rt.register_node(&c);
                let code = format!("__makeNode({})", c.id);
                if let Ok(v) = ctx.eval(Source::from_bytes(code.as_bytes())) {
                    return Ok(v);
                }
            }
        }
        Ok(JsValue::null())
    });
    reg!(ctx, "__node_nextSibling", 1, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            if let Some(c) = n.next_sibling() {
                rt.register_node(&c);
                let code = format!("__makeNode({})", c.id);
                if let Ok(v) = ctx.eval(Source::from_bytes(code.as_bytes())) {
                    return Ok(v);
                }
            }
        }
        Ok(JsValue::null())
    });
    reg!(ctx, "__node_prevSibling", 1, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            if let Some(c) = n.previous_sibling() {
                rt.register_node(&c);
                let code = format!("__makeNode({})", c.id);
                if let Ok(v) = ctx.eval(Source::from_bytes(code.as_bytes())) {
                    return Ok(v);
                }
            }
        }
        Ok(JsValue::null())
    });
    reg!(ctx, "__node_appendChild", 2, |_this, args, _ctx| {
        let parent_id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let child_id = args.get_or_undefined(1).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let (Some(p), Some(c)) = (rt.lookup_node(parent_id), rt.lookup_node(child_id)) {
            Node::append_child(&p, c);
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_removeChild", 2, |_this, args, _ctx| {
        let parent_id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let child_id = args.get_or_undefined(1).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let (Some(p), Some(c)) = (rt.lookup_node(parent_id), rt.lookup_node(child_id)) {
            Node::remove_child(&p, &c);
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_insertBefore", 3, |_this, args, _ctx| {
        let parent_id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let child_id = args.get_or_undefined(1).as_number().unwrap_or(0.0) as usize;
        let ref_id = args.get_or_undefined(2).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let (Some(p), Some(c)) = (rt.lookup_node(parent_id), rt.lookup_node(child_id)) {
            let r = if ref_id == 0 { None } else { rt.lookup_node(ref_id) };
            Node::insert_before(&p, c, r.as_ref());
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_replaceChild", 3, |_this, args, _ctx| {
        let parent_id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let new_id = args.get_or_undefined(1).as_number().unwrap_or(0.0) as usize;
        let old_id = args.get_or_undefined(2).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let (Some(p), Some(new_), Some(old)) = (rt.lookup_node(parent_id), rt.lookup_node(new_id), rt.lookup_node(old_id)) {
            Node::replace_child(&p, new_, &old);
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_clone", 2, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let deep = args.get_or_undefined(1).as_boolean().unwrap_or(false);
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            let clone = clone_node(&n, deep);
            rt.register_node(&clone);
            return Ok(JsValue::new(clone.id as f64));
        }
        Ok(JsValue::new(0.0))
    });
    reg!(ctx, "__node_remove", 1, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            if let Some(parent) = n.parent() {
                Node::remove_child(&parent, &n);
            }
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_insertAdjacentHTML", 3, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let pos = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let html = args
            .get_or_undefined(2)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            // Parse the HTML fragment.
            let parsed = crate::html::parser::parse(&format!("<html><body>{}</body></html>", html));
            let body = find_body(&parsed);
            if let Some(b) = body {
                let new_nodes: Vec<NodeRef> = b.children();
                let parent = match pos.as_str() {
                    "beforebegin" | "afterend" => n.parent(),
                    "afterbegin" | "beforeend" => Some(n.clone()),
                    _ => Some(n.clone()),
                };
                if let Some(parent) = parent {
                    for c in new_nodes {
                        rt.register_node(&c);
                        match pos.as_str() {
                            "beforebegin" => {
                                let ref_node = n.next_sibling();
                                Node::insert_before(&parent, c, ref_node.as_ref());
                            }
                            "afterbegin" => {
                                let ref_node = parent.first_child();
                                Node::insert_before(&parent, c, ref_node.as_ref());
                            }
                            "beforeend" => {
                                Node::append_child(&parent, c);
                            }
                            "afterend" => {
                                let ref_node = n.next_sibling();
                                let _ = ref_node;
                                // Insert after n: find n's next sibling, insert before it.
                                // If no next sibling, append to parent.
                                let next = n.next_sibling();
                                Node::insert_before(&parent, c, next.as_ref());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_contains", 2, |_this, args, _ctx| {
        let parent_id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let child_id = args.get_or_undefined(1).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let (Some(p), Some(c)) = (rt.lookup_node(parent_id), rt.lookup_node(child_id)) {
            let mut cur = Some(c);
            while let Some(n) = cur {
                if n.id == p.id {
                    return Ok(JsValue::new(true));
                }
                cur = n.parent();
            }
        }
        Ok(JsValue::new(false))
    });
    reg!(ctx, "__node_addEventListener", 3, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let event_type = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let handler = args.get_or_undefined(2).clone();
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            let hid = rt.register_handler();
            let name = format!("__h_{}", hid);
            ctx.global_object()
                .set(js_string!(name.as_str()), handler, false, ctx)
                .ok();
            n.add_event_listener(&event_type, hid, false);
            return Ok(JsValue::new(hid as f64));
        }
        Ok(JsValue::new(0.0))
    });
    reg!(ctx, "__node_removeEventListener", 2, |_this, args, _ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let event_type = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            n.remove_event_listeners(&event_type);
        }
        Ok(JsValue::undefined())
    });
    reg!(ctx, "__node_dispatchEvent", 2, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let event_type = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            let listeners = n.event_listeners(&event_type);
            for l in listeners {
                // Call the handler directly in this context (we already hold
                // the context lock from the outer evaluate()).
                let code = format!(
                    "(typeof __h_{} === 'function') ? __h_{}() : undefined;",
                    l.handler_id, l.handler_id
                );
                let _ = ctx.eval(Source::from_bytes(code.as_bytes()));
                let _ = ctx.run_jobs();
            }
        }
        Ok(JsValue::new(true))
    });
    reg!(ctx, "__node_querySelector", 2, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let sel = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            if let Some(found) = query_selector(&n, &sel) {
                rt.register_node(&found);
                let code = format!("__makeNode({})", found.id);
                if let Ok(v) = ctx.eval(Source::from_bytes(code.as_bytes())) {
                    return Ok(v);
                }
            }
        }
        Ok(JsValue::null())
    });
    reg!(ctx, "__node_querySelectorAll", 2, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let sel = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        let mut ids = Vec::new();
        if let Some(n) = rt.lookup_node(id) {
            for f in query_selector_all(&n, &sel) {
                rt.register_node(&f);
                ids.push(f.id);
            }
        }
        Ok(make_array(ctx, &ids))
    });
    reg!(ctx, "__node_getElementById", 2, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let target = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            if let Some(found) = Node::get_element_by_id(&n, &target) {
                rt.register_node(&found);
                let code = format!("__makeNode({})", found.id);
                if let Ok(v) = ctx.eval(Source::from_bytes(code.as_bytes())) {
                    return Ok(v);
                }
            }
        }
        Ok(JsValue::null())
    });
    reg!(ctx, "__node_getElementsByTagName", 2, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let tag = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        let mut ids = Vec::new();
        if let Some(n) = rt.lookup_node(id) {
            for f in Node::get_elements_by_tag_name(&n, &tag) {
                rt.register_node(&f);
                ids.push(f.id);
            }
        }
        Ok(make_array(ctx, &ids))
    });
    reg!(ctx, "__node_getElementsByClassName", 2, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let class = args
            .get_or_undefined(1)
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default();
        let rt = current_runtime();
        let mut ids = Vec::new();
        if let Some(n) = rt.lookup_node(id) {
            for f in Node::get_elements_by_class(&n, &class) {
                rt.register_node(&f);
                ids.push(f.id);
            }
        }
        Ok(make_array(ctx, &ids))
    });
    reg!(ctx, "__node_getBoundingClientRect", 1, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        if let Some(n) = rt.lookup_node(id) {
            let l = n.layout().unwrap_or_default();
            let obj = JsObject::with_object_proto(ctx.intrinsics());
            obj.set(js_string!("x"), JsValue::new(l.x as f64), false, ctx).ok();
            obj.set(js_string!("y"), JsValue::new(l.y as f64), false, ctx).ok();
            obj.set(js_string!("left"), JsValue::new(l.x as f64), false, ctx).ok();
            obj.set(js_string!("top"), JsValue::new(l.y as f64), false, ctx).ok();
            obj.set(js_string!("right"), JsValue::new((l.x + l.width) as f64), false, ctx).ok();
            obj.set(js_string!("bottom"), JsValue::new((l.y + l.height) as f64), false, ctx).ok();
            obj.set(js_string!("width"), JsValue::new(l.width as f64), false, ctx).ok();
            obj.set(js_string!("height"), JsValue::new(l.height as f64), false, ctx).ok();
            return Ok(obj.into());
        }
        Ok(JsValue::null())
    });
    reg!(ctx, "__node_attributes", 1, |_this, args, ctx| {
        let id = args.get_or_undefined(0).as_number().unwrap_or(0.0) as usize;
        let rt = current_runtime();
        let arr = JsArray::new(ctx).expect("JsArray::new");
        if let Some(n) = rt.lookup_node(id) {
            let kind = n.kind();
            if let NodeKind::Element(e) = kind {
                for name in e.attr_order.iter() {
                    if let Some(val) = e.attributes.get(name) {
                        let pair = JsObject::with_object_proto(ctx.intrinsics());
                        pair.set(js_string!("name"), s2v(name), false, ctx).ok();
                        pair.set(js_string!("value"), s2v(val), false, ctx).ok();
                        let _ = arr.push(pair, ctx);
                    }
                }
            }
        }
        Ok(arr.into())
    });
}

fn find_body(doc: &NodeRef) -> Option<NodeRef> {
    for c in doc.children() {
        if c.tag_name().map(|t| t.eq_ignore_ascii_case("html")).unwrap_or(false) {
            for cc in c.children() {
                if cc.tag_name().map(|t| t.eq_ignore_ascii_case("body")).unwrap_or(false) {
                    return Some(cc);
                }
            }
        }
    }
    None
}

fn clone_node(src: &NodeRef, deep: bool) -> NodeRef {
    let kind = src.kind();
    let new_node = match &kind {
        NodeKind::Element(e) => {
            let n = crate::html::dom::element(&e.tag);
            for name in &e.attr_order {
                if let Some(val) = e.attributes.get(name) {
                    n.set_attribute(name, val);
                }
            }
            n
        }
        NodeKind::Text(t) => crate::html::dom::text(t),
        NodeKind::Comment(c) => Node::new(NodeKind::Comment(c.clone())),
        _ => return crate::html::dom::document(),
    };
    if deep {
        for c in src.children() {
            let cloned = clone_node(&c, true);
            Node::append_child(&new_node, cloned);
        }
    }
    new_node
}

fn serialize_node(n: &NodeRef) -> String {
    let mut out = String::new();
    serialize_node_to(n, &mut out);
    out
}

fn serialize_subtree(n: &NodeRef) -> String {
    let mut out = String::new();
    for c in n.children() {
        serialize_node_to(&c, &mut out);
    }
    out
}

fn serialize_node_to(n: &NodeRef, out: &mut String) {
    let kind = n.kind();
    match &kind {
        NodeKind::Element(e) => {
            out.push('<');
            out.push_str(&e.tag);
            for name in &e.attr_order {
                if let Some(val) = e.attributes.get(name) {
                    out.push(' ');
                    out.push_str(name);
                    out.push_str("=\"");
                    out.push_str(&html_escape_attr(val));
                    out.push('"');
                }
            }
            out.push('>');
            if !crate::html::dom::is_void_element(&e.tag) {
                for c in n.children() {
                    serialize_node_to(&c, out);
                }
                out.push_str("</");
                out.push_str(&e.tag);
                out.push('>');
            }
        }
        NodeKind::Text(t) => {
            out.push_str(&html_escape_text(t));
        }
        NodeKind::Comment(c) => {
            out.push_str("<!--");
            out.push_str(c);
            out.push_str("-->");
        }
        _ => {}
    }
}

fn html_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

fn html_escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn query_selector(root: &NodeRef, sel: &str) -> Option<NodeRef> {
    let selector = crate::css::selector::Selector::parse(sel)?;
    let mut found = None;
    let mut done = false;
    Node::walk_ref(root, &mut |n| {
        if done {
            return false;
        }
        if selector.matches(n) {
            found = Some(n.clone());
            done = true;
            return false;
        }
        true
    });
    found
}

fn query_selector_all(root: &NodeRef, sel: &str) -> Vec<NodeRef> {
    let selector = match crate::css::selector::Selector::parse(sel) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    Node::walk_ref(root, &mut |n| {
        if selector.matches(n) {
            out.push(n.clone());
        }
        true
    });
    out
}
