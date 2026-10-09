//! Complete source commits before marking the operation applied.
use crate::{
    execute::Target,
    write::{StagedFile, failpoint, failpoint_result},
};
use std::{io, path::Path, thread};

struct Pending<'a> {
    path: &'a Path,
    staged: Option<StagedFile>,
}

type Worker<'scope> = io::Result<thread::ScopedJoinHandle<'scope, io::Result<()>>>;

pub(crate) fn commit_all(targets: &[Target], staged: Vec<Option<StagedFile>>) -> io::Result<()> {
    let mut pending: Vec<_> = targets
        .iter()
        .zip(staged)
        .map(|(target, staged)| Pending {
            path: &target.path,
            staged,
        })
        .collect();
    let Some((first, rest)) = pending.split_first_mut() else {
        return Ok(());
    };
    commit(first)?;
    failpoint("after_first_rename");
    commit_rest(rest)
}

pub(crate) fn commit_files(paths: &[&Path], staged: Vec<Option<StagedFile>>) -> io::Result<()> {
    let mut pending: Vec<_> = paths
        .iter()
        .zip(staged)
        .map(|(path, staged)| Pending { path, staged })
        .collect();
    commit_rest(&mut pending)
}

fn commit_rest(pending: &mut [Pending<'_>]) -> io::Result<()> {
    if pending.len() < 31 {
        return pending.iter_mut().try_for_each(commit);
    }
    let size = pending.len().div_ceil(16);
    thread::scope(|scope| {
        let workers: Vec<_> = pending
            .chunks_mut(size)
            .map(|chunk| {
                thread::Builder::new()
                    .spawn_scoped(scope, move || chunk.iter_mut().try_for_each(commit))
            })
            .collect();
        let results: Vec<_> = workers.into_iter().map(join_worker).collect();
        results.into_iter().collect()
    })
}

fn join_worker(worker: Worker<'_>) -> io::Result<()> {
    worker?
        .join()
        .map_err(|_| io::Error::other("commit worker panicked"))?
}

fn commit(pending: &mut Pending<'_>) -> io::Result<()> {
    failpoint_result("before_commit")?;
    match pending.staged.take() {
        Some(temp) => temp.commit(pending.path),
        None => match std::fs::remove_file(pending.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        },
    }
}
