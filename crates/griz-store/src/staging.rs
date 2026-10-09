//! Bounded staging workers complete before shared device synchronization.
#[cfg(target_vendor = "apple")]
use crate::batch_sync::finish_sync;
use crate::{
    batch_sync::BatchSync,
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
    let Some((first, rest)) = targets.split_first() else {
        return Ok(Vec::new());
    };
    let sync = BatchSync::default();
    let batch = uuid::Uuid::now_v7();
    let open = prepare_target(first, batch)?;
    mid_stage_failpoint(0)?;
    let mut staged = vec![open.map(|open| sync_stage(open, &sync)).transpose()?];
    let mut remaining: Vec<_> = rest.iter().collect();
    staged.extend(crate::parallel::map(&mut remaining, |target| {
        stage_target(target, batch, &sync)
    })?);
    #[cfg(target_vendor = "apple")]
    finish_sync(&sync)?;
    Ok(staged)
}

fn stage_target(
    target: &FileStage<'_>,
    batch: uuid::Uuid,
    sync: &BatchSync,
) -> io::Result<Option<StagedFile>> {
    prepare_target(target, batch)?
        .map(|open| sync_stage(open, sync))
        .transpose()
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
