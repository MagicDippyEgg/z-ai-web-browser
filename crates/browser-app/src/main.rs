//! Z Browser — a from-scratch web browser.
//!
//! See `README.md` and `docs/ARCHITECTURE.md` for an overview.

use browser_ui::Browser;
use std::env;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();
    log::info!("Starting Z Browser v{}", env!("CARGO_PKG_VERSION"));
    let mut browser = Browser::new();
    // If a URL was passed on the command line, navigate to it.
    let args: Vec<String> = env::args().collect();
    if let Some(url) = args.get(1) {
        browser.navigate_active(url);
    }
    browser.run();
}
