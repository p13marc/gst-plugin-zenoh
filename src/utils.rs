use std::sync::LazyLock;

use tokio::runtime;

pub static RUNTIME: LazyLock<runtime::Runtime> = LazyLock::new(|| {
    runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(1)
        .thread_name("gst-zenoh-runtime")
        .build()
        .unwrap()
});
