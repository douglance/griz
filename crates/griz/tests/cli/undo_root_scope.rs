//! Two repositories sharing one griz home, driven exactly as two independent
//! agent sessions would: an undo scoped to one repo's root must never write
//! into the other's files, even though the store holds every repository's
//! history in one sequence.

use serde_json::Value;
use std::{error::Error, path::PathBuf, process::Command};

type TestResult = Result<(), Box<dyn Error>>;

/// One repository: its own workspace, sharing `home` with any other `Repo`.
struct Repo {
    home: PathBuf,
    work: tempfile::TempDir,
}

impl Repo {
    fn new(home: &std::path::Path) -> Result<Self, Box<dyn Error>> {
        let work = tempfile::tempdir()?;
        std::fs::create_dir_all(work.path().join(".git"))?;
        Ok(Self {
            home: home.to_path_buf(),
            work,
        })
    }

    fn write(&self, name: &str, text: &str) -> Result<(), Box<dyn Error>> {
        std::fs::write(self.work.path().join(name), text)?;
        Ok(())
    }

    fn read(&self, name: &str) -> Result<String, Box<dyn Error>> {
        Ok(std::fs::read_to_string(self.work.path().join(name))?)
    }

    fn run(&self, args: &[&str]) -> Result<Value, Box<dyn Error>> {
        let output = Command::new(env!("CARGO_BIN_EXE_griz"))
            .args(args)
            .arg("--json")
            .current_dir(self.work.path())
            .env("GRIZ_HOME", &self.home)
            .env("XDG_DATA_HOME", self.home.join("xdg"))
            .env_remove("GRIZ_VERBOSITY")
            .env_remove("GRIZ_FAILPOINT")
            .output()?;
        let stdout = String::from_utf8(output.stdout)?;
        serde_json::from_str(&stdout).map_err(|error| {
            format!(
                "{error}: stdout was {stdout:?}, stderr {:?}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into()
        })
    }

    /// Runs a mutation with a purpose and key, returning its id.
    fn id(&self, args: &[&str], key: &str) -> Result<String, Box<dyn Error>> {
        let mut full = args.to_vec();
        full.extend(["--purpose", "test", "--idempotency-key", key]);
        let json = self.run(&full)?;
        assert_eq!(json["outcome"], "passed", "{json}");
        Ok(json["id"].as_str().ok_or("no id")?.to_string())
    }

    /// Applies one single-file replace, returning its operation id.
    fn edit(&self, name: &str, from: &str, to: &str, key: &str) -> Result<String, Box<dyn Error>> {
        let ops = format!(
            r#"[{{"op":"replace","path":"{name}","find":{{"text":"{from}"}},"replace":"{to}"}}]"#
        );
        let plan = self.id(&["plan", "--ops", &ops], &format!("{key}-plan"))?;
        self.id(&["apply", &plan], key)
    }
}

#[test]
fn undo_since_scoped_to_one_repos_root_never_writes_another_repos_file() -> TestResult {
    let home = tempfile::tempdir()?;
    let repo_a = Repo::new(home.path())?;
    let repo_b = Repo::new(home.path())?;

    repo_a.write("a.rs", "fn f() { 1; }\n")?;
    repo_b.write("b.rs", "fn g() { 2; }\n")?;

    let op_a = repo_a.edit("a.rs", "1;", "11;", "a1")?;
    // repo_b applies after repo_a, so its operation's sequence follows
    // op_a's: an unscoped `since` scan would otherwise sweep it in.
    repo_b.edit("b.rs", "2;", "22;", "b1")?;

    let restore = repo_a.run(&[
        "undo",
        "--since",
        &op_a,
        "--purpose",
        "roll back repo a",
        "--idempotency-key",
        "undo-a",
    ])?;
    assert_eq!(restore["outcome"], "passed", "{restore}");

    assert_eq!(repo_a.read("a.rs")?, "fn f() { 1; }\n");
    assert_eq!(
        repo_b.read("b.rs")?,
        "fn g() { 22; }\n",
        "an undo scoped to repo a's root must never touch repo b's file"
    );
    Ok(())
}

#[test]
fn undo_of_an_operation_from_another_repos_root_is_refused() -> TestResult {
    let home = tempfile::tempdir()?;
    let repo_a = Repo::new(home.path())?;
    let repo_b = Repo::new(home.path())?;

    repo_b.write("b.rs", "fn g() { 2; }\n")?;
    let op_b = repo_b.edit("b.rs", "2;", "22;", "b1")?;

    let refused = repo_a.run(&[
        "undo",
        &op_b,
        "--verbosity",
        "warn",
        "--purpose",
        "undo from the wrong repo",
        "--idempotency-key",
        "undo-wrong-repo",
    ])?;
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert!(
        refused["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("root"),
        "{refused}"
    );
    assert_eq!(
        repo_b.read("b.rs")?,
        "fn g() { 22; }\n",
        "a cross-repo undo must never write"
    );
    Ok(())
}
