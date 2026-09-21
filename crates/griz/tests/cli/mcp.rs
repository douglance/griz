//! MCP serves each command as its own direct tool.

use serde_json::{Value, json};
use std::{
    error::Error,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

type TestResult = Result<(), Box<dyn Error>>;

/// A griz `--mcp` process, spoken to over stdio JSON-RPC.
struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    _home: tempfile::TempDir,
}

impl Mcp {
    fn start() -> Result<Self, Box<dyn Error>> {
        let home = tempfile::tempdir()?;
        let mut child = Command::new(env!("CARGO_BIN_EXE_griz"))
            .arg("--mcp")
            .env("GRIZ_HOME", home.path())
            .env("XDG_DATA_HOME", home.path().join("xdg"))
            .env_remove("GRIZ_VERBOSITY")
            .env_remove("GRIZ_FAILPOINT")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        Ok(Self {
            child,
            stdin,
            stdout,
            next_id: 1,
            _home: home,
        })
    }

    /// Sends a request and returns its parsed response.
    fn call(&mut self, method: &str, params: &Value) -> Result<Value, Box<dyn Error>> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let mut line = String::new();
        self.stdout.read_line(&mut line)?;
        Ok(serde_json::from_str(&line)?)
    }

    /// Sends a notification; the protocol has no response to read.
    fn notify(&mut self, method: &str, params: &Value) -> Result<(), Box<dyn Error>> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    fn send(&mut self, message: &Value) -> Result<(), Box<dyn Error>> {
        writeln!(self.stdin, "{message}")?;
        self.stdin.flush()?;
        Ok(())
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mcp_serves_each_command_as_its_own_tool() -> TestResult {
    let mut mcp = Mcp::start()?;
    let init = mcp.call(
        "initialize",
        &json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "griz-cli-test", "version": "0.0.0" },
        }),
    )?;
    assert!(init.get("result").is_some(), "{init}");
    mcp.notify("notifications/initialized", &json!({}))?;
    let listed = mcp.call("tools/list", &json!({}))?;
    let tools = listed["result"]["tools"]
        .as_array()
        .ok_or_else(|| format!("no tools in {listed}"))?;
    let mut names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    names.sort_unstable();
    let mut expected = vec![
        "absorb", "apply", "diff", "find", "get", "log", "plan", "read", "select", "undo",
    ];
    expected.sort_unstable();
    assert_eq!(names, expected, "{listed}");
    Ok(())
}
