//! Bounded write waves complete before shared device synchronization.
use crate::{
    batch_sync::{BatchSync, finish_sync},
    execute::Target,
    write::{OpenStage, StagedFile, failpoint, failpoint_result, prepare_bytes, sync_stage},
};
use std::{io, path::Path};

type Staged = Vec<Option<StagedFile>>;

pub(crate) struct FileStage<'a> {
    pub(crate) path: &'a Path,
    pub(crate) bytes: Option<&'a [u8]>,
}

pub(crate) fn stage_all(targets: &[Target]) -> io::Result<Staged> {
    let files: Vec<_> = targets
        .iter()
        .map(|target| FileStage {
            path: &target.path,
            bytes: target.text.as_deref().map(str::as_bytes),
        })
        .collect();
    let staged = stage_files(&files)?;
    reject_directory_destinations(targets)?;
    Ok(staged)
}

pub(crate) fn stage_files(targets: &[FileStage<'_>]) -> io::Result<Staged> {
    let sync = BatchSync::default();
    let batch = uuid::Uuid::now_v7();
    let mut staged = Vec::with_capacity(targets.len());
    for (index, wave) in targets.chunks(16).enumerate() {
        let mut open = prepare_wave(wave, index * 16, batch)?;
        let ready = crate::parallel::map(&mut open, |file| {
            file.take().map(|open| sync_stage(open, &sync)).transpose()
        })?;
        staged.extend(ready);
    }
    finish_sync(&sync)?;
    Ok(staged)
}

fn prepare_wave(
    targets: &[FileStage<'_>],
    start: usize,
    batch: uuid::Uuid,
) -> io::Result<Vec<Option<OpenStage>>> {
    let Some((first, rest)) = targets.split_first() else {
        return Ok(Vec::new());
    };
    let mut open = vec![prepare_target(first, batch)?];
    mid_stage_failpoint(start)?;
    let mut remaining: Vec<_> = rest.iter().collect();
    open.extend(crate::parallel::map(&mut remaining, |target| {
        prepare_target(target, batch)
    })?);
    Ok(open)
}

fn mid_stage_failpoint(index: usize) -> io::Result<()> {
    if index == 0 {
        failpoint("mid_stage");
        failpoint_result("mid_stage_error")?;
    }
    Ok(())
}

fn reject_directory_destinations(targets: &[Target]) -> io::Result<()> {
    for target in targets {
        match std::fs::metadata(&target.path) {
            Ok(metadata) if metadata.is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::IsADirectory,
                    "a planned file destination became a directory during staging",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn prepare_target(target: &FileStage<'_>, batch: uuid::Uuid) -> io::Result<Option<OpenStage>> {
    target
        .bytes
        .map(|bytes| prepare_bytes(target.path, bytes, batch))
        .transpose()
}
