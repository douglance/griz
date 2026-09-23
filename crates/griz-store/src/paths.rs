//! Physical parent directories identify atomic rename destinations.

use std::{
    io,
    path::{Path, PathBuf},
};

/// Resolves existing directory aliases, retaining a missing directory suffix.
///
/// # Errors
/// Returns an error for inaccessible, non-directory, or dangling-link prefixes.
pub fn directory_path(path: &Path) -> io::Result<PathBuf> {
    let mut prefix = path.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(&prefix) {
            Ok(mut directory) => {
                require_directory(&directory)?;
                directory.extend(missing.into_iter().rev());
                return Ok(directory);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(missing_component(&prefix, error)?);
                prefix = prefix
                    .parent()
                    .filter(|path| !path.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf();
            }
            Err(error) => return Err(error),
        }
    }
}

fn follow_file_links(resolver: &mut PathResolver, path: &Path) -> io::Result<PathBuf> {
    let mut entry = resolver.resolve(path)?;
    let mut visited = std::collections::BTreeSet::new();
    while let Some(target) = file_link(&entry)? {
        if !visited.insert(entry.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("symbolic-link cycle: {}", entry.display()),
            ));
        }
        let parent = entry.parent().unwrap_or_else(|| Path::new("."));
        entry = resolver.resolve(&parent.join(target))?;
    }
    match std::fs::canonicalize(&entry) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(entry),
        Err(error) => Err(error),
    }
}

pub(crate) fn file_link(path: &Path) -> io::Result<Option<PathBuf>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::read_link(path).map(Some),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn require_directory(path: &Path) -> io::Result<()> {
    if std::fs::metadata(path)?.is_dir() {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::NotADirectory,
        format!("not a directory: {}", path.display()),
    ))
}

fn missing_component(path: &Path, original: io::Error) -> io::Result<std::ffi::OsString> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
        Ok(_) => return Err(original),
    }
    path.file_name()
        .map(std::ffi::OsStr::to_os_string)
        .ok_or(original)
}

/// Resolves a write's parent directory, retaining its final directory entry.
///
/// # Errors
/// Returns an error when the parent contains an invalid directory prefix.
pub fn destination_path(path: &Path) -> io::Result<PathBuf> {
    PathResolver::default().resolve(path)
}

/// Directory and file-alias identities retained for one batch of paths.
#[derive(Default)]
pub struct PathResolver {
    directories: std::collections::BTreeMap<PathBuf, PathBuf>,
    files: std::collections::BTreeMap<PathBuf, PathBuf>,
}

impl PathResolver {
    /// Resolves links and existing filename spelling, caching each path for this batch.
    ///
    /// # Errors
    /// Returns an error for inaccessible paths or symbolic-link cycles.
    pub fn resolve_file(&mut self, path: &Path) -> io::Result<PathBuf> {
        if let Some(target) = self.files.get(path) {
            return Ok(target.clone());
        }
        let target = follow_file_links(self, path)?;
        self.files.insert(path.to_path_buf(), target.clone());
        Ok(target)
    }

    /// Resolves a destination, looking up each submitted parent only once.
    ///
    /// # Errors
    /// Returns an error when the parent contains an invalid directory prefix.
    pub fn resolve(&mut self, path: &Path) -> io::Result<PathBuf> {
        let Some(name) = path.file_name() else {
            return directory_path(path);
        };
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !self.directories.contains_key(parent) {
            self.directories
                .insert(parent.to_path_buf(), directory_path(parent)?);
        }
        Ok(self.directories[parent].join(name))
    }
}
