//! Parallel iteration with rayon when the `parallel` feature is enabled
//! (the default); sequential otherwise (e.g. for WebAssembly).

#[cfg(feature = "parallel")]
pub(crate) fn map<T: Send, R: Send>(items: Vec<T>, f: impl Fn(T) -> R + Sync + Send) -> Vec<R> {
    use rayon::prelude::*;
    items.into_par_iter().map(f).collect()
}

#[cfg(not(feature = "parallel"))]
pub(crate) fn map<T: Send, R: Send>(items: Vec<T>, f: impl Fn(T) -> R + Sync + Send) -> Vec<R> {
    items.into_iter().map(f).collect()
}
