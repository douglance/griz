//! Request-local paths without changing the worker process directory.
use std::{cell::RefCell, path::PathBuf};
tokio::task_local! { pub(crate) static DIRECTORY: PathBuf; }
thread_local! { static BLOCKING_DIRECTORY: RefCell<Option<PathBuf>> = const { RefCell::new(None) }; }
pub(crate) fn current_dir() -> std::io::Result<PathBuf> {
    DIRECTORY
        .try_with(Clone::clone)
        .ok()
        .or_else(|| BLOCKING_DIRECTORY.with(|directory| directory.borrow().clone()))
        .map_or_else(std::env::current_dir, Ok)
}
pub(crate) fn blocking<T>(directory: Option<PathBuf>, work: impl FnOnce() -> T) -> T {
    let previous = BLOCKING_DIRECTORY.with(|value| value.replace(directory));
    let _guard = DirectoryGuard(previous);
    work()
}
struct DirectoryGuard(Option<PathBuf>);
impl Drop for DirectoryGuard {
    fn drop(&mut self) {
        BLOCKING_DIRECTORY.with(|value| value.replace(self.0.take()));
    }
}
