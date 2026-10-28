//! T6 gate: fault injection at the wire. Every tool answers
//! malformed input with an `isError` domain result — never a
//! protocol error, never a panic. Plus the D1 regression
//! class: the surface's own schema generator cannot produce a
//! tool schema without `properties`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use torori::server::tool_specs;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "torori-faults", "version": "0"},
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    root: std::path::PathBuf,
}

fn spawn() -> Session {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("torori-faults-{}-{n}", std::process::id()));
    let data = format!("{}/data", env!("CARGO_MANIFEST_DIR"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_torori"))
        .args(["--data", &data, "--root", root.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn torori");
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    Session {
        child,
        stdin,
        stdout,
        root,
    }
}

impl Session {
    fn request(&mut self, id: u64, method: &str, mut params: Value) -> Value {
        params
            .as_object_mut()
            .unwrap()
            .insert("_meta".into(), meta());
        let line = serde_json::to_string(
            &json!({"id": id, "jsonrpc": "2.0", "method": method, "params": params}),
        )
        .unwrap();
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
        let mut buf = String::new();
        self.stdout.read_line(&mut buf).unwrap();
        serde_json::from_str(buf.trim()).unwrap_or_else(|_| panic!("non-JSON output: {buf}"))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn every_schema_declares_properties() {
    // D1: the generator-side guarantee — every spec in this
    // surface carries an object schema WITH properties.
    for spec in tool_specs() {
        assert_eq!(
            spec.schema["type"], "object",
            "{} schema must be an object",
            spec.name
        );
        assert!(
            spec.schema.get("properties").is_some_and(|p| p.is_object()),
            "{} schema must declare properties (D1 regression class)",
            spec.name
        );
    }
}

#[test]
fn fault_injection_every_tool_fails_as_is_error() {
    let mut s = spawn();
    let specs = tool_specs();
    for (i, spec) in specs.iter().enumerate() {
        let resp = s.request(
            100 + i as u64,
            "tools/call",
            json!({"name": spec.name, "arguments": {}}),
        );
        assert!(
            resp.get("error").is_none(),
            "{}: domain failure leaked as a protocol error: {resp}",
            spec.name
        );
        let required = spec
            .schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        if required {
            assert_eq!(
                resp["result"]["isError"], true,
                "{}: missing required args must be isError, got {resp}",
                spec.name
            );
            // The failure carries the fleet's typed body.
            let text = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
            let body: Value = serde_json::from_str(text).unwrap_or(Value::Null);
            assert!(
                body["error"]["code"].is_string(),
                "{}: typed error body required, got {text}",
                spec.name
            );
        }
    }
    // Unknown tool: -32602 by fleet convention.
    let resp = s.request(
        999,
        "tools/call",
        json!({"name": "torori_nonexistent", "arguments": {}}),
    );
    assert_eq!(resp["error"]["code"], -32602, "{resp}");
    // Wrong-typed arguments also fail as isError, not panics.
    let resp = s.request(
        1000,
        "tools/call",
        json!({"name": "torori_catalogue_get", "arguments": {"slug": 42}}),
    );
    assert!(
        resp.get("error").is_none() && resp["result"]["isError"] == true,
        "wrong-typed slug must be isError: {resp}"
    );
}
