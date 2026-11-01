//! The JS runtime: owns a Boa `Context` plus a registry of node references
//! and timer/event handlers.
//!
//! Architecture note: Boa's `NativeFunction`s receive a `&mut Context` but
//! not the runtime itself. To bridge, we install a thread-local pointer to
//! the runtime during `evaluate()` / `call_handler()` / `pump()`. Native
//! functions retrieve the runtime via `current_runtime()`.

use super::dom_api::install_dom;
use super::globals::install_globals;
use boa_engine::{Context, JsResult, JsValue as BoaJsValue, Source};
use parking_lot::Mutex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::html::dom::NodeRef;

static NEXT_HANDLER_ID: AtomicUsize = AtomicUsize::new(1);

pub type HandlerId = usize;

thread_local! {
    /// Pointer to the JsRuntime currently running on this thread. Set by
    /// `JsRuntime::evaluate` and friends; read by native functions.
    pub(crate) static CURRENT_RT: RefCell<Option<*const JsRuntime>> = const { RefCell::new(None) };
    /// Pending handler calls queued from within native functions. Drained by
    /// `evaluate()` after the main code completes.
    pub(crate) static PENDING_CALLS: RefCell<Vec<HandlerId>> = const { RefCell::new(Vec::new()) };
}

/// Retrieve the runtime for the current thread. Only valid inside a native
/// function called from `JsRuntime::evaluate` / `call_handler` / `pump`.
pub fn current_runtime() -> &'static JsRuntime {
    CURRENT_RT.with(|slot| {
        let ptr = slot.borrow().expect("no current JsRuntime");
        // SAFETY: The runtime is alive for the duration of `evaluate()` (or
        // `call_handler` / `pump`), which is the only time native functions
        // are called. We extend the lifetime to 'static as a lie that is
        // safe in practice.
        unsafe { &*ptr }
    })
}

/// A pending timer (setTimeout / setInterval).
pub struct Timer {
    pub id: usize,
    pub fire_at: Instant,
    pub interval: Option<Duration>,
    pub handler_id: HandlerId,
    pub code: Option<String>,
}

/// The JavaScript runtime for a single page.
pub struct JsRuntime {
    /// The Boa context. Boa is not Sync.
    pub(crate) ctx: Mutex<Context>,
    /// Map from node id to NodeRef. JS holds nodes by id; we resolve back to
    /// the real Arc here.
    pub(crate) nodes: Mutex<HashMap<usize, NodeRef>>,
    /// The document node. JS uses id 0 to refer to the document.
    pub(crate) document: Mutex<Option<NodeRef>>,
    /// Map from handler id to handler kind. The actual JS function is stored
    /// as a global property `__h_<id>` on the Boa global object.
    pub(crate) handlers: Mutex<HashMap<HandlerId, ()>>,
    /// Pending timers.
    pub(crate) timers: Mutex<Vec<Timer>>,
    /// Pending micro-tasks (Promise continuations). Stored as JS source.
    pub(crate) microtasks: Mutex<Vec<String>>,
    /// Pending macro-tasks.
    pub(crate) macrotasks: Mutex<Vec<String>>,
    /// The page's origin, for same-origin checks.
    pub(crate) origin: String,
    /// The page URL (for fetch resolution).
    pub(crate) base_url: String,
}

#[derive(Debug, Clone)]
pub struct JsValue(pub String);

#[derive(Debug, Clone)]
pub struct JsError(pub String);

impl JsRuntime {
    pub fn new(origin: String, base_url: String) -> Self {
        let mut ctx = Context::default();
        install_globals(&mut ctx);
        let rt = Self {
            ctx: Mutex::new(ctx),
            nodes: Mutex::new(HashMap::new()),
            document: Mutex::new(None),
            handlers: Mutex::new(HashMap::new()),
            timers: Mutex::new(Vec::new()),
            microtasks: Mutex::new(Vec::new()),
            macrotasks: Mutex::new(Vec::new()),
            origin,
            base_url,
        };
        install_dom(&rt);
        rt
    }

    /// Set the document node. The document is also accessible via id 0
    /// (which is what JS uses for `document`).
    pub fn set_document(&self, doc: NodeRef) {
        *self.document.lock() = Some(doc.clone());
        self.nodes.lock().insert(0, doc);
    }

    /// Register a node so JS can find it by id.
    pub fn register_node(&self, node: &NodeRef) {
        self.nodes.lock().insert(node.id, node.clone());
    }

    pub fn lookup_node(&self, id: usize) -> Option<NodeRef> {
        self.nodes.lock().get(&id).cloned()
    }

    /// Allocate a new handler id.
    pub fn register_handler(&self) -> HandlerId {
        let id = NEXT_HANDLER_ID.fetch_add(1, Ordering::Relaxed);
        self.handlers.lock().insert(id, ());
        id
    }

    pub fn remove_handler(&self, id: HandlerId) {
        self.handlers.lock().remove(&id);
        // Also delete the global function property.
        let name = format!("__h_{}", id);
        let _ = self.with_context(|ctx| {
            let _ = ctx.eval(Source::from_bytes(
                format!("delete globalThis['{}'];", name).as_bytes(),
            ));
        });
    }

    /// Run a closure with mutable access to the Boa context. The closure runs
    /// with `CURRENT_RT` set to this runtime, so any native functions it
    /// invokes can find the runtime via `current_runtime()`.
    pub fn with_context<R>(&self, f: impl FnOnce(&mut Context) -> R) -> R {
        CURRENT_RT.with(|slot| *slot.borrow_mut() = Some(self as *const JsRuntime));
        let result;
        let mut guard = self.ctx.lock();
        result = f(&mut guard);
        CURRENT_RT.with(|slot| *slot.borrow_mut() = None);
        result
    }

    /// Evaluate a script. Returns the result as a string.
    pub fn evaluate(&self, code: &str) -> Result<String, JsError> {
        self.with_context(|ctx| {
            let src = Source::from_bytes(code.as_bytes());
            let result: JsResult<BoaJsValue> = ctx.eval(src);
            // Run pending Promise jobs (microtasks) before returning.
            let _ = ctx.run_jobs();
            // Drain any handler calls queued by native functions during this
            // evaluation. We run them now while we still hold the lock.
            let pending: Vec<HandlerId> = PENDING_CALLS.with(|q| q.borrow_mut().drain(..).collect());
            for id in pending {
                let code = format!(
                    "(typeof __h_{} === 'function') ? __h_{}() : undefined;",
                    id, id
                );
                let _ = ctx.eval(Source::from_bytes(code.as_bytes()));
                let _ = ctx.run_jobs();
            }
            match result {
                Ok(v) => {
                    // If the result is a string, return its raw contents (no quotes).
                    let s = if let Some(string) = v.as_string() {
                        string.to_std_string_escaped()
                    } else {
                        format!("{}", v.display())
                    };
                    Ok(s)
                }
                Err(e) => {
                    let s = format!("{}", e);
                    Err(JsError(s))
                }
            }
        })
    }

    /// Call a registered handler by id with no arguments.
    pub fn call_handler(&self, id: HandlerId) -> Result<String, JsError> {
        // We can't call evaluate() here because evaluate() calls with_context()
        // which acquires the context lock. If we're already inside a native
        // function (which holds the lock), this would deadlock.
        //
        // Instead, we use a different thread-local: while inside a native
        // function, the lock is already held by the calling evaluate(). We
        // detect this case via CURRENT_RT being Some — but we still need the
        // lock. To avoid the deadlock, we use try_lock and fall back to a
        // thread-local evaluation queue.
        //
        // Simpler: call_handler is only called from outside a native function
        // (e.g. from the page's event dispatch loop, which runs evaluate()
        // in its own scope). For calls from within native functions (e.g.
        // dispatchEvent firing nested handlers), use call_handler_in_ctx.
        if CURRENT_RT.with(|s| s.borrow().is_some()) {
            // We're inside a native function — the lock is held by the
            // outer evaluate(). Use the thread-local context.
            return self.call_handler_via_thread_local(id);
        }
        let code = format!(
            "(typeof __h_{} === 'function') ? __h_{}() : undefined;",
            id, id
        );
        self.evaluate(&code)
    }

    fn call_handler_via_thread_local(&self, id: HandlerId) -> Result<String, JsError> {
        // The current thread already has the lock; we just need to run code
        // in the same context. We can't easily do this without re-entering
        // with_context (which would deadlock). The cleanest way is to queue
        // the call as a microtask and let the outer evaluate() process it
        // via run_jobs(). But Boa's run_jobs is called at the end of
        // evaluate, so by the time call_handler returns, the queued job
        // will run.
        //
        // For simplicity, we use a thread-local "pending calls" queue.
        PENDING_CALLS.with(|q| q.borrow_mut().push(id));
        Ok(String::new())
    }

    /// Schedule a timer.
    pub fn schedule_timer(
        &self,
        delay: Duration,
        repeat: bool,
        handler_id: HandlerId,
        code: Option<String>,
    ) -> usize {
        let id = NEXT_HANDLER_ID.fetch_add(1, Ordering::Relaxed);
        let timer = Timer {
            id,
            fire_at: Instant::now() + delay,
            interval: if repeat { Some(delay) } else { None },
            handler_id,
            code,
        };
        self.timers.lock().push(timer);
        id
    }

    pub fn cancel_timer(&self, id: usize) {
        self.timers.lock().retain(|t| t.id != id);
    }

    /// Pump the event loop: fire any due timers and run pending microtasks.
    pub fn pump(&self) {
        let now = Instant::now();
        let mut due: Vec<Timer> = Vec::new();
        {
            let mut timers = self.timers.lock();
            let mut i = 0;
            while i < timers.len() {
                if timers[i].fire_at <= now {
                    let t = timers.remove(i);
                    due.push(t);
                } else {
                    i += 1;
                }
            }
        }
        for t in due {
            if let Some(code) = &t.code {
                let _ = self.evaluate(code);
            } else {
                let _ = self.call_handler(t.handler_id);
            }
            if let Some(interval) = t.interval {
                self.timers.lock().push(Timer {
                    id: t.id,
                    fire_at: Instant::now() + interval,
                    interval: Some(interval),
                    handler_id: t.handler_id,
                    code: t.code,
                });
            }
        }
        let micros: Vec<String> = self.microtasks.lock().drain(..).collect();
        for m in micros {
            let _ = self.evaluate(&m);
        }
        let macros: Vec<String> = self.macrotasks.lock().drain(..).collect();
        for m in macros {
            let _ = self.evaluate(&m);
        }
    }

    pub fn queue_microtask(&self, code: String) {
        self.microtasks.lock().push(code);
    }

    pub fn queue_macrotask(&self, code: String) {
        self.macrotasks.lock().push(code);
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }
}
