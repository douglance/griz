//! MCP serves each command as its own direct tool.

mod addressing;
mod search_errors;
mod transport;

use serde_json::{Value, json};
use std::{
    error::Error,
    io::Write,
    process::{Child, ChildStdin, Command, Stdio},
};

type TestResult = Result<(), Box<dyn Error>>;

/// A griz `--mcp` process, spoken to over stdio JSON-RPC.
struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: std::sync::mpsc::Receiver<std::io::Result<String>>,
    next_id: u64,
    _home: tempfile::TempDir,
}

impl Mcp {
    /// Starts a process and completes the initialize handshake.
    fn ready() -> Result<Self, Box<dyn Error>> {
        let mut mcp = Self::start()?;
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
        Ok(mcp)
    }

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
        let stdout = transport::lines(child.stdout.take().ok_or("no stdout")?);
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
        let line = self
            .stdout
            .recv_timeout(std::time::Duration::from_secs(10))??;
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
    let mut mcp = Mcp::ready()?;
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

/// Fetches the `plan` tool's published `inputSchema` from `tools/list`.
fn plan_input_schema(mcp: &mut Mcp) -> Result<Value, Box<dyn Error>> {
    let listed = mcp.call("tools/list", &json!({}))?;
    let tools = listed["result"]["tools"]
        .as_array()
        .ok_or_else(|| format!("no tools in {listed}"))?;
    let plan = tools
        .iter()
        .find(|tool| tool["name"] == "plan")
        .ok_or_else(|| format!("no plan tool in {listed}"))?;
    Ok(plan["inputSchema"].clone())
}

/// Collects every `$ref` string value anywhere in `value`.
fn collect_refs<'a>(value: &'a Value, refs: &mut Vec<&'a str>) {
    match value {
        Value::Object(map) => {
            if let Some(r) = map.get("$ref").and_then(Value::as_str) {
                refs.push(r);
            }
            map.values().for_each(|v| collect_refs(v, refs));
        }
        Value::Array(items) => items.iter().for_each(|v| collect_refs(v, refs)),
        _ => {}
    }
}

/// Every `$ref` in `schema` must resolve as a JSON pointer within it.
fn assert_every_ref_resolves(schema: &Value) {
    let mut refs = Vec::new();
    collect_refs(schema, &mut refs);
    assert!(!refs.is_empty(), "expected at least one $ref in {schema}");
    for r in refs {
        let pointer = r.strip_prefix('#').unwrap_or(r);
        assert!(
            schema.pointer(pointer).is_some(),
            "$ref {r} does not resolve in {schema}"
        );
    }
}

#[test]
fn plan_publishes_a_typed_ops_schema() -> TestResult {
    let mut mcp = Mcp::ready()?;
    let schema = plan_input_schema(&mut mcp)?;

    for name in [
        "ops",
        "patch",
        "root",
        "purpose",
        "idempotency_key",
        "expect_edits",
        "expect_files",
        "verbosity",
    ] {
        assert!(
            schema["properties"][name].is_object(),
            "missing {name} in {schema}"
        );
    }

    let branches = schema["properties"]["ops"]["items"]["oneOf"]
        .as_array()
        .ok_or_else(|| format!("ops.items has no oneOf in {schema}"))?;
    assert!(
        branches.iter().any(|b| b["type"] == "string"),
        "no plain-string branch in {branches:?}"
    );
    let op_variants = branches
        .iter()
        .find_map(|b| b["oneOf"].as_array())
        .ok_or_else(|| format!("no Op oneOf branch in {branches:?}"))?;
    let mut kinds: Vec<&str> = op_variants
        .iter()
        .filter_map(|variant| variant["properties"]["op"]["const"].as_str())
        .collect();
    kinds.sort_unstable();
    let mut expected = ["create", "delete", "insert", "move", "replace"];
    expected.sort_unstable();
    assert_eq!(kinds, expected, "{op_variants:?}");

    assert_every_ref_resolves(&schema);
    Ok(())
}

#[test]
fn plan_schema_publishes_pattern_fields() -> TestResult {
    let mut mcp = Mcp::ready()?;
    let schema = plan_input_schema(&mut mcp)?;

    let op_variants = schema["properties"]["ops"]["items"]["oneOf"]
        .as_array()
        .ok_or_else(|| format!("ops.items has no oneOf in {schema}"))?
        .iter()
        .find_map(|b| b["oneOf"].as_array())
        .ok_or_else(|| format!("no Op oneOf branch in {schema}"))?;
    let replace = op_variants
        .iter()
        .find(|variant| variant["properties"]["op"]["const"] == "replace")
        .ok_or_else(|| format!("no replace variant in {op_variants:?}"))?;
    for name in ["pattern", "target"] {
        assert!(
            replace["properties"][name].is_object(),
            "missing {name} on replace in {replace}"
        );
    }

    assert_every_ref_resolves(&schema);
    Ok(())
}

#[test]
fn plan_schema_publishes_workspace_edit_fields() -> TestResult {
    let mut mcp = Mcp::ready()?;
    let schema = plan_input_schema(&mut mcp)?;

    for name in ["workspace_edit", "position_encoding"] {
        assert!(
            schema["properties"][name].is_object(),
            "missing {name} in {schema}"
        );
    }
    assert!(
        schema["properties"]["workspace_edit"]["properties"]["documentChanges"]["type"]
            .as_array()
            .is_some_and(|types| types.iter().any(|t| t == "array")),
        "{schema}"
    );

    assert_every_ref_resolves(&schema);
    Ok(())
}

#[test]
fn plan_call_accepts_a_typed_op_object() -> TestResult {
    let mut mcp = Mcp::ready()?;
    let called = mcp.call(
        "tools/call",
        &json!({
            "name": "plan",
            "arguments": {
                "ops": [{ "op": "create", "path": "typed.txt", "text": "hi\n" }],
                "purpose": "test",
                "idempotency_key": "typed-op-object",
            },
        }),
    )?;
    let content = called["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| format!("no content text in {called}"))?;
    let body: Value = serde_json::from_str(content)?;
    assert_eq!(body["outcome"], "passed", "{body}");
    Ok(())
}
