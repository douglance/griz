//! Finishes operations an interrupted process left `applying`.
//!
//! Every text an operation writes is stored before its first rename, so an
//! interrupted operation can always roll forward: files still at their old
//! fingerprint get the new text, files already at the new fingerprint are
//! done, and files at neither are someone else's change and are left alone.

use crate::{
    FileWrite, Operation, OperationState, Store, StoreError, execute::read_current,
    write::write_atomic,
};

/// Rolls every unfinished operation forward.
///
/// # Errors
/// Returns an error when the journal cannot be read or updated.
pub fn recover(store: &Store) -> Result<(), StoreError> {
    for op in store.unfinished_operations()? {
        recover_one(store, op)?;
    }
    Ok(())
}

fn recover_one(store: &Store, mut op: Operation) -> Result<(), StoreError> {
    let paths: Vec<_> = op.files.iter().map(|file| file.path.clone()).collect();
    let _locks = store.lock_paths(&paths)?;
    let mut conflicts = Vec::new();
    for file in &op.files {
        if !roll_forward(store, file)? {
            conflicts.push(file.path.clone());
        }
    }
    op.conflicts.extend(conflicts);
    op.recovered = true;
    op.state = OperationState::Applied;
    store.save_operation(&op)
}

/// Brings one file to its written state. Returns `false` when the file is at
/// neither fingerprint, or could not be written, and so was left alone.
fn roll_forward(store: &Store, file: &FileWrite) -> Result<bool, StoreError> {
    let (_, current_hash) = read_current(&file.path)?;
    if current_hash == file.after_hash {
        return Ok(true);
    }
    if current_hash != file.before_hash {
        return Ok(false);
    }
    let written = match &file.after_hash {
        Some(hash) => write_atomic(&file.path, &store.get_blob(hash)?),
        None => std::fs::remove_file(&file.path),
    };
    Ok(written.is_ok())
}
