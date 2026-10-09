//! Bounded ordered work, joining every worker before returning an error.
use std::{io, thread};

pub(crate) fn map<T, R, E, F>(items: &mut [T], work: F) -> Result<Vec<R>, E>
where
    T: Send,
    R: Send,
    E: Send + From<io::Error>,
    F: Fn(&mut T) -> Result<R, E> + Sync,
{
    if items.len() < 8 {
        return items.iter_mut().map(work).collect();
    }
    let size = items.len().div_ceil(4);
    thread::scope(|scope| {
        let workers: Vec<_> = items
            .chunks_mut(size)
            .map(|chunk| spawn_worker(scope, chunk, &work))
            .collect();
        let results: Vec<_> = workers.into_iter().map(join_worker).collect();
        results
            .into_iter()
            .collect::<Result<Vec<_>, E>>()
            .map(|chunks| chunks.into_iter().flatten().collect())
    })
}

fn join_worker<R, E>(
    worker: io::Result<thread::ScopedJoinHandle<'_, Result<Vec<R>, E>>>,
) -> Result<Vec<R>, E>
where
    E: From<io::Error>,
{
    worker?
        .join()
        .map_err(|_| io::Error::other("batch worker panicked"))?
}

fn spawn_worker<'scope, 'env, T, R, E, F>(
    scope: &'scope thread::Scope<'scope, 'env>,
    chunk: &'scope mut [T],
    work: &'scope F,
) -> io::Result<thread::ScopedJoinHandle<'scope, Result<Vec<R>, E>>>
where
    T: Send,
    R: Send + 'scope,
    E: Send + 'scope,
    F: Fn(&mut T) -> Result<R, E> + Sync,
{
    thread::Builder::new().spawn_scoped(scope, move || chunk.iter_mut().map(work).collect())
}
