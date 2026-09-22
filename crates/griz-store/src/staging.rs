//! Bounded staging concurrency; source renames remain ordered in execute.

use crate::{
    execute::Target,
    write::{StagedFile, failpoint, failpoint_result, stage},
};
use std::{io, thread};

type Staged = Vec<Option<StagedFile>>;
type Worker<'scope> = io::Result<thread::ScopedJoinHandle<'scope, io::Result<Staged>>>;

/// Waits for every stage and its sync before returning any batch to the journal.
pub(crate) fn stage_all(targets: &[Target]) -> io::Result<Staged> {
    let writes = targets
        .iter()
        .filter(|target| target.text.is_some())
        .take(32)
        .count();
    if writes < 32 {
        return stage_slice(targets, 0);
    }
    let size = targets.len().div_ceil(4);
    thread::scope(|scope| {
        let workers: Vec<_> = targets
            .chunks(size)
            .enumerate()
            .map(|(index, chunk)| {
                thread::Builder::new().spawn_scoped(scope, move || stage_slice(chunk, index * size))
            })
            .collect();
        let results: Vec<_> = workers.into_iter().map(join_worker).collect();
        let batches = results.into_iter().collect::<io::Result<Vec<_>>>()?;
        Ok(batches.into_iter().flatten().collect())
    })
}

fn join_worker(worker: Worker<'_>) -> io::Result<Staged> {
    worker?
        .join()
        .map_err(|_| io::Error::other("staging worker panicked"))?
}

fn stage_slice(targets: &[Target], start: usize) -> io::Result<Staged> {
    let mut staged = Vec::with_capacity(targets.len());
    for (offset, target) in targets.iter().enumerate() {
        let temp = target
            .text
            .as_deref()
            .map(|text| stage(&target.path, text))
            .transpose()?;
        staged.push(temp);
        mid_stage_failpoint(start + offset)?;
    }
    Ok(staged)
}

fn mid_stage_failpoint(index: usize) -> io::Result<()> {
    if index == 0 {
        failpoint("mid_stage");
        failpoint_result("mid_stage_error")?;
    }
    Ok(())
}
