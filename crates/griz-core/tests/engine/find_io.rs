//! Search failures must not look like complete empty or partial results.

use griz_core::{FindQuery, find};
use std::{error::Error, fs, path::Path};

type TestResult = Result<(), Box<dyn Error>>;

fn query(path: &Path) -> FindQuery {
    FindQuery {
        paths: vec![path.to_path_buf()],
        literal: Some("needle".into()),
        limit: 1,
        ..FindQuery::default()
    }
}

#[test]
fn missing_search_paths_refuse_empty_and_partial_results() -> TestResult {
    let tree = tempfile::tempdir()?;
    let good = tree.path().join("good.txt");
    let missing = tree.path().join("missing");
    fs::write(&good, "needle")?;
    for paths in [
        vec![missing.clone()],
        vec![good.clone(), missing.clone()],
        vec![missing.clone(), good],
    ] {
        let result = find(&FindQuery {
            paths,
            ..query(tree.path())
        });
        let Err(error) = result else {
            panic!("incomplete search succeeded: {result:?}")
        };
        assert!(error.contains("missing"), "{error}");
    }
    Ok(())
}

#[test]
fn empty_directories_and_non_utf8_files_remain_searchable() -> TestResult {
    let tree = tempfile::tempdir()?;
    assert_eq!(find(&query(tree.path()))?.total, 0);
    let binary = tree.path().join("binary.dat");
    fs::write(&binary, [0xff, 0xfe])?;
    assert_eq!(find(&query(&binary))?.total, 0);
    fs::write(tree.path().join("good.txt"), "needle")?;
    let result = find(&query(tree.path()))?;
    assert_eq!((result.total, result.files), (1, 1));
    Ok(())
}

#[cfg(unix)]
struct Denied {
    path: std::path::PathBuf,
    permissions: fs::Permissions,
}

#[cfg(unix)]
impl Denied {
    fn new(path: &Path) -> Result<Self, Box<dyn Error>> {
        use std::os::unix::fs::PermissionsExt;
        let denied = Self {
            path: path.into(),
            permissions: fs::metadata(path)?.permissions(),
        };
        fs::set_permissions(path, fs::Permissions::from_mode(0o000))?;
        Ok(denied)
    }
}

#[cfg(unix)]
impl Drop for Denied {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, self.permissions.clone());
    }
}

#[cfg(unix)]
#[test]
fn unreadable_text_files_refuse_partial_results() -> TestResult {
    let tree = tempfile::tempdir()?;
    let denied = tree.path().join("z-denied.txt");
    fs::write(tree.path().join("a-good.txt"), "needle")?;
    fs::write(&denied, "needle")?;
    let _permissions = Denied::new(&denied)?;
    // A privileged process cannot exercise a permission refusal.
    if fs::read_to_string(&denied).is_ok() {
        eprintln!("permission-refusal scenario unavailable for this process");
        return Ok(());
    }
    let result = find(&query(tree.path()));
    let Err(error) = result else {
        panic!("unreadable search succeeded: {result:?}")
    };
    assert!(
        error.contains("z-denied.txt") && error.contains("reading"),
        "{error}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn unreadable_directories_refuse_partial_results() -> TestResult {
    let tree = tempfile::tempdir()?;
    let denied = tree.path().join("z-denied");
    fs::create_dir(&denied)?;
    fs::write(tree.path().join("a-good.txt"), "needle")?;
    fs::write(denied.join("hidden.txt"), "needle")?;
    let _permissions = Denied::new(&denied)?;
    if fs::read_dir(&denied).is_ok() {
        eprintln!("permission-refusal scenario unavailable for this process");
        return Ok(());
    }
    let result = find(&query(tree.path()));
    let Err(error) = result else {
        panic!("incomplete traversal succeeded: {result:?}")
    };
    assert!(error.contains("z-denied"), "{error}");
    Ok(())
}
