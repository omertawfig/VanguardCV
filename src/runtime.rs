//! Shared Tokio runtime for asynchronous AI work.
//!
//! GPUI owns the main thread and has its own executors, but the Antigravity
//! client uses reqwest/hyper, which require a Tokio reactor. The desktop app
//! creates this runtime once at startup and spawns AI runs on it; streaming
//! events are consumed by GPUI through a runtime-independent mpsc channel.

use std::sync::OnceLock;

use tokio::runtime::{Builder, Handle, Runtime};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Initializes the shared runtime (idempotent).
pub fn init() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("vanguardcv-ai")
            .enable_all()
            .build()
            .expect("failed to build the shared Tokio runtime")
    })
}

/// Handle to the shared runtime, creating it on demand.
pub fn handle() -> Handle {
    init().handle().clone()
}
