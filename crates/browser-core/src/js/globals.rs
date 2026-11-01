//! Browser globals: `console`, `setTimeout`, `setInterval`, `fetch`, `alert`,
//! `window`, etc. Installed into the Boa context.

use boa_engine::object::{FunctionObjectBuilder, JsObject};
use boa_engine::property::Attribute;
use boa_engine::{js_string, Context, JsArgs, JsResult, JsValue, NativeFunction};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use super::runtime::current_runtime;

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

thread_local! {
    /// In-memory localStorage for the current page.
    static LOCAL_STORAGE: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Install the browser globals into the Boa context.
pub fn install_globals(ctx: &mut Context) {
    // Clear localStorage for this thread (in case the context is reused).
    LOCAL_STORAGE.with(|s| s.borrow_mut().clear());

    let console = build_console(ctx);
    ctx.register_global_property(js_string!("console"), console, Attribute::all())
        .expect("register console");

    ctx.register_global_callable(
        js_string!("setTimeout"),
        1,
        NativeFunction::from_fn_ptr(|_this, args, ctx| {
            let rt = current_runtime();
            let delay_ms = args.get_or_undefined(1).as_number().unwrap_or(0.0) as u64;
            let code_or_fn = args.get_or_undefined(0).clone();
            let id = rt.register_handler();
            let name = format!("__h_{}", id);
            ctx.global_object()
                .set(js_string!(name.as_str()), code_or_fn, false, ctx)?;
            rt.schedule_timer(Duration::from_millis(delay_ms), false, id, None);
            Ok(JsValue::new(id as f64))
        }),
    )
    .expect("register setTimeout");

    ctx.register_global_callable(
        js_string!("setInterval"),
        1,
        NativeFunction::from_fn_ptr(|_this, args, ctx| {
            let rt = current_runtime();
            let delay_ms = args.get_or_undefined(1).as_number().unwrap_or(0.0) as u64;
            let code_or_fn = args.get_or_undefined(0).clone();
            let id = rt.register_handler();
            let name = format!("__h_{}", id);
            ctx.global_object()
                .set(js_string!(name.as_str()), code_or_fn, false, ctx)?;
            rt.schedule_timer(Duration::from_millis(delay_ms), true, id, None);
            Ok(JsValue::new(id as f64))
        }),
    )
    .expect("register setInterval");

    ctx.register_global_callable(
        js_string!("clearTimeout"),
        1,
        NativeFunction::from_fn_ptr(|_this, args, _ctx| {
            let rt = current_runtime();
            if let Some(id) = args.get_or_undefined(0).as_number() {
                rt.cancel_timer(id as usize);
            }
            Ok(JsValue::undefined())
        }),
    )
    .expect("register clearTimeout");

    ctx.register_global_callable(
        js_string!("clearInterval"),
        1,
        NativeFunction::from_fn_ptr(|_this, args, _ctx| {
            let rt = current_runtime();
            if let Some(id) = args.get_or_undefined(0).as_number() {
                rt.cancel_timer(id as usize);
            }
            Ok(JsValue::undefined())
        }),
    )
    .expect("register clearInterval");

    ctx.register_global_callable(
        js_string!("alert"),
        1,
        NativeFunction::from_fn_ptr(|_this, args, _ctx| {
            if let Some(s) = args.get_or_undefined(0).as_string() {
                log::info!("[alert] {}", s.to_std_string_escaped());
            }
            Ok(JsValue::undefined())
        }),
    )
    .expect("register alert");

    let global = ctx.global_object().clone();
    ctx.register_global_property(js_string!("window"), global.clone(), Attribute::all())
        .expect("register window");
    // `self` is an alias for `window` (used by many scripts and web workers)
    ctx.register_global_property(js_string!("self"), global, Attribute::all())
        .expect("register self");
    // `top` and `parent` are also aliases for `window` in a non-iframe context
    ctx.register_global_property(js_string!("top"), ctx.global_object().clone(), Attribute::all())
        .ok();
    ctx.register_global_property(js_string!("parent"), ctx.global_object().clone(), Attribute::all())
        .ok();
    // `frames` is also an alias for `window`
    ctx.register_global_property(js_string!("frames"), ctx.global_object().clone(), Attribute::all())
        .ok();

    // localStorage — backed by a thread-local HashMap.
    let storage = JsObject::with_object_proto(ctx.intrinsics());
    let get_fn = make_fn(
        ctx,
        "getItem",
        NativeFunction::from_fn_ptr(|_this, args, _ctx| {
            let key = args
                .get_or_undefined(0)
                .as_string()
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_default();
            let v = LOCAL_STORAGE.with(|s| s.borrow().get(&key).cloned());
            match v {
                Some(v) => Ok(s2v(&v)),
                None => Ok(JsValue::null()),
            }
        }),
    );
    let set_fn = make_fn(
        ctx,
        "setItem",
        NativeFunction::from_fn_ptr(|_this, args, _ctx| {
            let key = args
                .get_or_undefined(0)
                .as_string()
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_default();
            let value = args
                .get_or_undefined(1)
                .as_string()
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_default();
            LOCAL_STORAGE.with(|s| s.borrow_mut().insert(key, value));
            Ok(JsValue::undefined())
        }),
    );
    let remove_fn = make_fn(
        ctx,
        "removeItem",
        NativeFunction::from_fn_ptr(|_this, args, _ctx| {
            let key = args
                .get_or_undefined(0)
                .as_string()
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_default();
            LOCAL_STORAGE.with(|s| s.borrow_mut().remove(&key));
            Ok(JsValue::undefined())
        }),
    );
    let clear_fn = make_fn(
        ctx,
        "clear",
        NativeFunction::from_fn_ptr(|_this, _args, _ctx| {
            LOCAL_STORAGE.with(|s| s.borrow_mut().clear());
            Ok(JsValue::undefined())
        }),
    );
    storage
        .set(js_string!("getItem"), JsValue::from(get_fn), false, ctx)
        .ok();
    storage
        .set(js_string!("setItem"), JsValue::from(set_fn), false, ctx)
        .ok();
    storage
        .set(
            js_string!("removeItem"),
            JsValue::from(remove_fn),
            false,
            ctx,
        )
        .ok();
    storage
        .set(js_string!("clear"), JsValue::from(clear_fn), false, ctx)
        .ok();
    ctx.register_global_property(js_string!("localStorage"), storage, Attribute::all())
        .expect("register localStorage");

    // fetch — perform a synchronous HTTP request and return a Promise.
    let fetch_fn = make_fn(
        ctx,
        "fetch",
        NativeFunction::from_fn_ptr(|_this, args, ctx| {
            let rt = current_runtime();
            let url = match args.get_or_undefined(0).as_string() {
                Some(s) => s.to_std_string_escaped(),
                None => return Ok(JsValue::undefined()),
            };
            let base = rt.base_url().to_string();
            let abs = resolve_url(&base, &url);
            let client = crate::net::HttpClient::default();
            let resp = match client.fetch(&abs) {
                Ok(r) => r,
                Err(e) => {
                    let err = s2v(&format!("{}", e));
                    let promise =
                        boa_engine::object::builtins::JsPromise::reject(
                            boa_engine::JsError::from_opaque(err),
                            ctx,
                        )?;
                    return Ok(promise.into());
                }
            };
            let status = resp.status as f64;
            let body = String::from_utf8_lossy(&resp.body).to_string();
            let body_for_text = body.clone();
            let body_for_json = body.clone();
            let response = JsObject::with_object_proto(ctx.intrinsics());
            response
                .set(js_string!("status"), JsValue::new(status), false, ctx)
                .ok();
            response
                .set(
                    js_string!("ok"),
                    JsValue::new(status >= 200.0 && status < 300.0),
                    false,
                    ctx,
                )
                .ok();
            response
                .set(js_string!("url"), s2v(&abs), false, ctx)
                .ok();
            let text_fn = make_fn(
                ctx,
                "text",
                // SAFETY: closure captures only owned 'static data.
                unsafe {
                    NativeFunction::from_closure(move |_this, _args, ctx| {
                        let promise = boa_engine::object::builtins::JsPromise::resolve(
                            s2v(&body_for_text),
                            ctx,
                        )?;
                        Ok(promise.into())
                    })
                },
            );
            response
                .set(js_string!("text"), JsValue::from(text_fn), false, ctx)
                .ok();
            let json_fn = make_fn(
                ctx,
                "json",
                // SAFETY: closure captures only owned 'static data.
                unsafe {
                    NativeFunction::from_closure(move |_this, _args, ctx| {
                        let promise = match ctx.eval(boa_engine::Source::from_bytes(
                            format!("({})", body_for_json).as_bytes(),
                        )) {
                            Ok(v) => boa_engine::object::builtins::JsPromise::resolve(v, ctx)?,
                            Err(_) => boa_engine::object::builtins::JsPromise::reject(
                                boa_engine::JsError::from_opaque(s2v("invalid JSON")),
                                ctx,
                            )?,
                        };
                        Ok(promise.into())
                    })
                },
            );
            response
                .set(js_string!("json"), JsValue::from(json_fn), false, ctx)
                .ok();
            let promise = boa_engine::object::builtins::JsPromise::resolve(
                JsValue::from(response),
                ctx,
            )?;
            Ok(promise.into())
        }),
    );
    ctx.register_global_property(js_string!("fetch"), fetch_fn, Attribute::all())
        .expect("register fetch");
}

fn build_console(ctx: &mut Context) -> JsObject {
    let console = JsObject::with_object_proto(ctx.intrinsics());
    let log_fn = make_fn(
        ctx,
        "log",
        NativeFunction::from_fn_ptr(|_this, args, _ctx| {
            let parts: Vec<String> = args
                .iter()
                .map(|v| {
                    v.as_string()
                        .map(|s| s.to_std_string_escaped())
                        .unwrap_or_else(|| format!("{}", v.display()))
                })
                .collect();
            log::info!("[console] {}", parts.join(" "));
            Ok(JsValue::undefined())
        }),
    );
    let warn_fn = make_fn(
        ctx,
        "warn",
        NativeFunction::from_fn_ptr(|_this, args, _ctx| {
            let parts: Vec<String> = args
                .iter()
                .map(|v| {
                    v.as_string()
                        .map(|s| s.to_std_string_escaped())
                        .unwrap_or_else(|| format!("{}", v.display()))
                })
                .collect();
            log::warn!("[console] {}", parts.join(" "));
            Ok(JsValue::undefined())
        }),
    );
    let err_fn = make_fn(
        ctx,
        "error",
        NativeFunction::from_fn_ptr(|_this, args, _ctx| {
            let parts: Vec<String> = args
                .iter()
                .map(|v| {
                    v.as_string()
                        .map(|s| s.to_std_string_escaped())
                        .unwrap_or_else(|| format!("{}", v.display()))
                })
                .collect();
            log::error!("[console] {}", parts.join(" "));
            Ok(JsValue::undefined())
        }),
    );
    console
        .set(js_string!("log"), JsValue::from(log_fn.clone()), false, ctx)
        .ok();
    console
        .set(js_string!("info"), JsValue::from(log_fn.clone()), false, ctx)
        .ok();
    console
        .set(js_string!("warn"), JsValue::from(warn_fn), false, ctx)
        .ok();
    console
        .set(js_string!("error"), JsValue::from(err_fn), false, ctx)
        .ok();
    console
        .set(js_string!("debug"), JsValue::from(log_fn), false, ctx)
        .ok();
    console
}

fn resolve_url(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("data:") {
        return href.to_string();
    }
    match url::Url::parse(base) {
        Ok(b) => match b.join(href) {
            Ok(u) => u.to_string(),
            Err(_) => href.to_string(),
        },
        Err(_) => href.to_string(),
    }
}
