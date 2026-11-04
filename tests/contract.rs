//! Wire-level contract tests: spawn the real binary, speak
//! NDJSON, correlate by JSON-RPC id (rmcp dispatches pipelined
//! requests concurrently and answers out of order legally —
//! WS2 W1 §3.3).

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

static SEQ: AtomicU64 = AtomicU64::new(0);

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    root: std::path::PathBuf,
}

fn meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "torori-contract", "version": "0"},
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

fn spawn() -> Session {
    spawn_with(&[], &[])
}

fn spawn_with(extra_args: &[&str], envs: &[(&str, &str)]) -> Session {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("torori-contract-{}-{n}", std::process::id()));
    let data = format!("{}/data", env!("CARGO_MANIFEST_DIR"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_torori"));
    command
        .args(["--data", &data, "--root", root.to_str().unwrap()])
        .args(extra_args)
        // Isolate the launch from the test machine's own profile
        // selectors and config files (D-934 precedence inputs).
        .env_remove("TORORI_TOOL_PROFILE")
        .env_remove("TORORI_CONFIG")
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (key, value) in envs {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("spawn torori");
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
    fn send(&mut self, frame: &Value) {
        let line = serde_json::to_string(frame).unwrap();
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn send_raw(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Read one response frame (every harness frame sent here
    /// produces exactly one response; notifications produce none
    /// and are never sent mid-sequence by these tests).
    fn read(&mut self) -> Value {
        let mut buf = String::new();
        self.stdout.read_line(&mut buf).unwrap();
        serde_json::from_str(buf.trim()).unwrap_or_else(|_| panic!("non-JSON output: {buf}"))
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        let mut params = params;
        params
            .as_object_mut()
            .unwrap()
            .insert("_meta".into(), meta());
        self.send(&json!({"id": id, "jsonrpc": "2.0", "method": method, "params": params}));
        self.read()
    }

    fn call(&mut self, id: u64, name: &str, args: Value) -> Value {
        self.request(id, "tools/call", json!({"name": name, "arguments": args}))
    }

    /// Extract a tool result's text payload as JSON.
    fn payload(resp: &Value) -> Value {
        let text = resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text content in {resp}"));
        serde_json::from_str(text).unwrap_or_else(|_| panic!("payload not JSON: {text}"))
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
fn discover_advertises_only_the_modern_version() {
    let mut s = spawn();
    let resp = s.request(1, "server/discover", json!({}));
    let supported = &resp["result"]["supportedVersions"];
    assert_eq!(supported, &json!(["2026-07-28"]), "discover: {resp}");
}

#[test]
fn legacy_initialize_is_refused_and_the_server_stays_up() {
    let mut s = spawn();
    s.send(&json!({"id": 1, "jsonrpc": "2.0", "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "legacy", "version": "0"}}}));
    let resp = s.read();
    assert_eq!(resp["error"]["code"], -32022, "refusal: {resp}");
    assert_eq!(resp["error"]["data"]["supported"], json!(["2026-07-28"]));
    // The same session still serves modern frames afterwards.
    let list = s.request(2, "tools/list", json!({}));
    assert!(list["result"]["tools"].is_array(), "list: {list}");
}

#[test]
fn every_tool_is_annotated_with_a_properties_schema() {
    // The full surface (D-934): the default launch now serves
    // the core profile only, so the 19-tool annotation sweep
    // launches the explicit full profile.
    let mut s = spawn_with(&["--profile", "full"], &[]);
    let list = s.request(1, "tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    // All stages: 1 + 4 + 4 + 5 + 3 + 2 = 19 tools.
    assert_eq!(tools.len(), 19, "tools: {tools:?}");
    for tool in tools {
        assert_eq!(tool["inputSchema"]["type"], "object", "tool: {tool}");
        assert!(
            tool["inputSchema"]["properties"].is_object(),
            "properties present on 100% of schemas: {tool}"
        );
        for hint in ["readOnlyHint", "destructiveHint", "idempotentHint"] {
            assert!(
                tool["annotations"][hint].is_boolean(),
                "annotation {hint} present: {tool}"
            );
        }
    }
    let status = tools.iter().find(|t| t["name"] == "torori_status").unwrap();
    assert_eq!(status["annotations"]["readOnlyHint"], true);
    let transition = tools
        .iter()
        .find(|t| t["name"] == "torori_catalogue_transition")
        .unwrap();
    assert_eq!(transition["annotations"]["readOnlyHint"], false);
    assert_eq!(transition["annotations"]["destructiveHint"], false);
}

#[test]
fn status_call_returns_server_counts() {
    let mut s = spawn();
    let resp = s.call(1, "torori_status", json!({}));
    assert_ne!(resp["result"]["isError"], true, "status call: {resp}");
    let payload = Session::payload(&resp);
    assert_eq!(payload["name"], "torori");
    assert_eq!(payload["protocol"], "2026-07-28");
    assert_eq!(payload["tool_profile"], "core");
    assert!(payload["counts"]["catalogue_entries"].is_number());
}

#[test]
fn unknown_tool_is_a_protocol_error() {
    let mut s = spawn();
    let resp = s.call(1, "torori_nope", json!({}));
    assert_eq!(resp["error"]["code"], -32602, "unknown tool: {resp}");
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown tool")
    );
}

#[test]
fn unknown_method_and_malformed_lines_get_protocol_errors() {
    let mut s = spawn();
    let resp = s.request(1, "resources/list", json!({}));
    assert_eq!(resp["error"]["code"], -32601, "unknown method: {resp}");
    s.send_raw("this is not json");
    let resp = s.read();
    assert_eq!(resp["error"]["code"], -32700, "parse error: {resp}");
    assert_eq!(resp["id"], Value::Null);
    s.send_raw("[1,2,3]");
    let resp = s.read();
    assert_eq!(resp["error"]["code"], -32600, "non-object: {resp}");
}

#[test]
fn notification_is_dropped_unexecuted_and_session_continues() {
    let mut s = spawn();
    // Notification-shaped tools/call: dropped, never executed.
    s.send(&json!({"jsonrpc": "2.0", "method": "tools/call",
        "params": {"name": "torori_status", "arguments": {}}}));
    // Give the server a beat; the next response must be the
    // response to id 1, not the notification's.
    std::thread::sleep(Duration::from_millis(200));
    let resp = s.request(1, "tools/list", json!({}));
    assert_eq!(resp["id"], 1);
    assert!(resp["result"]["tools"].is_array());
}

#[test]
fn oversized_line_is_refused_not_fatal() {
    let mut s = spawn();
    let big = "x".repeat(4 * 1024 * 1024 + 16);
    s.send_raw(&format!("{{\"id\":1,\"pad\":\"{big}\"}}"));
    let resp = s.read();
    assert_eq!(resp["error"]["code"], -32700, "oversized: {resp}");
    let list = s.request(2, "tools/list", json!({}));
    assert!(list["result"]["tools"].is_array(), "still up: {list}");
}

// ── D-934 tool profiles, wire level ─────────────────────────────

fn tool_names(list: &Value) -> Vec<&str> {
    list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect()
}

#[test]
fn default_profile_serves_only_the_core_surface() {
    let mut s = spawn();
    let list = s.request(1, "tools/list", json!({}));
    assert_eq!(
        tool_names(&list),
        vec![
            "torori_status",
            "torori_catalogue_search",
            "torori_catalogue_get",
            "torori_catalogue_stats",
            "torori_catalogue_transition",
        ]
    );
}

#[test]
fn non_profile_tool_is_unknown_to_the_process() {
    let mut s = spawn();
    let resp = s.call(1, "torori_sfx_render", json!({"instrument": "impact"}));
    assert_eq!(resp["error"]["code"], -32602, "non-profile call: {resp}");
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown tool")
    );
    // A core tool in the same session still answers.
    let resp = s.call(2, "torori_status", json!({}));
    assert_ne!(resp["result"]["isError"], true, "status call: {resp}");
}

#[test]
fn profile_argument_and_env_select_packs_with_argument_winning() {
    let mut s = spawn_with(&["--profile", "bundles"], &[]);
    let list = s.request(1, "tools/list", json!({}));
    assert_eq!(tool_names(&list).len(), 10, "bundles list: {list}");
    assert!(tool_names(&list).contains(&"torori_pair_validate"));

    let mut s = spawn_with(&[], &[("TORORI_TOOL_PROFILE", "voice")]);
    let list = s.request(1, "tools/list", json!({}));
    assert_eq!(tool_names(&list).len(), 8, "voice list: {list}");
    let status = s.call(2, "torori_status", json!({}));
    assert_eq!(Session::payload(&status)["tool_profile"], "core+voice");

    let mut s = spawn_with(&["--profile", "sfx"], &[("TORORI_TOOL_PROFILE", "voice")]);
    let list = s.request(1, "tools/list", json!({}));
    assert_eq!(tool_names(&list).len(), 6, "arg-over-env list: {list}");
    assert!(tool_names(&list).contains(&"torori_sfx_render"));
}

#[test]
fn config_file_selects_a_pack() {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let config = std::env::temp_dir().join(format!(
        "torori-contract-config-{}-{n}.toml",
        std::process::id()
    ));
    std::fs::write(&config, "tool_profile = \"library\"\n").unwrap();
    let mut s = spawn_with(&["--config", config.to_str().unwrap()], &[]);
    let list = s.request(1, "tools/list", json!({}));
    assert_eq!(tool_names(&list).len(), 9, "config list: {list}");
    assert!(tool_names(&list).contains(&"torori_library_search"));
    let _ = std::fs::remove_file(&config);
}

#[test]
fn unknown_pack_launch_fails_closed() {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("torori-failclosed-{}-{n}", std::process::id()));
    let data = format!("{}/data", env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO_BIN_EXE_torori"))
        .args([
            "--data",
            &data,
            "--root",
            root.to_str().unwrap(),
            "--profile",
            "voic",
        ])
        .env_remove("TORORI_TOOL_PROFILE")
        .env_remove("TORORI_CONFIG")
        .stdin(Stdio::null())
        .output()
        .expect("run torori with an unknown pack");
    assert!(!output.status.success(), "status: {}", output.status);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("configuration_invalid"), "stderr: {stderr}");
    assert!(stderr.contains("unknown pack 'voic'"), "stderr: {stderr}");
    assert!(
        stderr.contains("valid: core, full, bundles, comp, library, sfx, voice"),
        "stderr: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
