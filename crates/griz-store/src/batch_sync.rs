//! File synchronization followed by a shared Apple device flush.
#[cfg(target_vendor = "apple")]
use std::{collections::BTreeMap, os::unix::fs::MetadataExt, sync::Mutex};
use std::{fs::File, io};

#[derive(Default)]
pub(crate) struct BatchSync {
    #[cfg(target_vendor = "apple")]
    devices: Mutex<BTreeMap<u64, File>>,
}

pub(crate) fn sync_file(batch: &BatchSync, file: &File) -> io::Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        kernel_sync(file)?;
        let device = file.metadata()?.dev();
        let mut devices = batch
            .devices
            .lock()
            .map_err(|_| io::Error::other("batch sync lock poisoned"))?;
        if let std::collections::btree_map::Entry::Vacant(entry) = devices.entry(device) {
            entry.insert(file.try_clone()?);
        }
        Ok(())
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        let _ = batch;
        file.sync_all()
    }
}

pub(crate) fn finish_sync(batch: &BatchSync) -> io::Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        let devices = batch
            .devices
            .lock()
            .map_err(|_| io::Error::other("batch sync lock poisoned"))?;
        for file in devices.values() {
            crate::write::failpoint_result("before_batch_full_sync")?;
            file.sync_all()?;
            crate::write::failpoint_result("after_batch_full_sync")?;
        }
    }
    #[cfg(not(target_vendor = "apple"))]
    let _ = batch;
    Ok(())
}

#[cfg(target_vendor = "apple")]
fn kernel_sync(file: &File) -> io::Result<()> {
    loop {
        match rustix::fs::fsync(file) {
            Err(rustix::io::Errno::INTR) => {}
            result => return result.map_err(Into::into),
        }
    }
}
