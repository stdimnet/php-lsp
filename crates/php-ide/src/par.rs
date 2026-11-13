//! Parallel iteration with rayon when the `parallel` feature is enabled
//! (the default); sequential otherwise (e.g. for WebAssembly).

/// Stack size of the threads php-ide and php-lsp start: deeply nested
/// syntax trees (long `.` chains in generated code) are walked recursively.
pub const STACK_SIZE: usize = 256 * 1024 * 1024;

#[cfg(feature = "parallel")]
pub(crate) fn map<T: Send, R: Send>(items: Vec<T>, f: impl Fn(T) -> R + Sync + Send) -> Vec<R> {
    use rayon::prelude::*;
    pool().install(|| items.into_par_iter().map(f).collect())
}

/// The thread pool for parallel work, with large stacks (the global pool
/// has the 2 MB default).
#[cfg(feature = "parallel")]
fn pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .stack_size(STACK_SIZE / 4)
            .thread_name(|i| format!("php-ide-{i}"))
            .build()
            .expect("thread pool")
    })
}

#[cfg(not(feature = "parallel"))]
pub(crate) fn map<T: Send, R: Send>(items: Vec<T>, f: impl Fn(T) -> R + Sync + Send) -> Vec<R> {
    items.into_iter().map(f).collect()
}
