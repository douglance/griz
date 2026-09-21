//! Runs the griz binary against an isolated state directory and workspace.

use serde_json::Value;
use std::{error::Error, path::PathBuf, process::Command};

pub type TestResult = Result<(), Box<dyn Error>>;

/// An isolated griz home and workspace.
pub struct Griz {
    pub work: tempfile::TempDir,
    home: tempfile::TempDir,
}

/// Exit status and parsed JSON output of one invocation.
pub struct Run {
    pub code: Option<i32>,
    pub json: Value,
}

impl Griz {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let griz = Self {
            work: tempfile::tempdir()?,
            home: tempfile::tempdir()?,
        };
        std::fs::create_dir_all(griz.work.path().join(".git"))?;
        Ok(griz)
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.work.path().join(name)
    }

    pub fn write(&self, name: &str, text: &str) -> Result<(), Box<dyn Error>> {
        let path = self.path(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn read(&self, name: &str) -> Result<String, Box<dyn Error>> {
        Ok(std::fs::read_to_string(self.path(name))?)
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_griz"));
        command
            .args(args)
            .arg("--json")
            .current_dir(self.work.path())
            .env("GRIZ_HOME", self.home.path())
            .env("XDG_DATA_HOME", self.home.path().join("xdg"))
            .env_remove("GRIZ_VERBOSITY")
            .env_remove("GRIZ_FAILPOINT");
        command
    }

    pub fn run(&self, args: &[&str]) -> Result<Run, Box<dyn Error>> {
        let output = self.command(args).output()?;
        let stdout = String::from_utf8(output.stdout)?;
        let json = serde_json::from_str(&stdout).map_err(|e| {
            format!(
                "{e}: stdout was {stdout:?}, stderr {:?}",
                String::from_utf8_lossy(&output.stderr)
            )
        })?;
        Ok(Run {
            code: output.status.code(),
            json,
        })
    }

    /// Runs a mutation with a purpose and key, returning its id.
    pub fn id(&self, args: &[&str], key: &str) -> Result<String, Box<dyn Error>> {
        let mut full = args.to_vec();
        full.extend(["--purpose", "test", "--idempotency-key", key]);
        let run = self.run(&full)?;
        assert_eq!(run.json["outcome"], "passed", "{}", run.json);
        Ok(run.json["id"].as_str().ok_or("no id")?.to_string())
    }
}
