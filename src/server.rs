//! The rmcp server: tool surface and handler dispatch.
//!
//! The surface is built stage by stage (brief §7); every tool
//! ships in its first stage with full schema + behaviour
//! annotations (tiger-style-mcp Rule 5: annotations from birth,
//! declared access matching detected access). Domain failures
//! return as `isError` results carrying the typed refusal payload
//! (`{"error":{"code","message"}}`); protocol faults (unknown
//! tool) are `-32602`. Dispatch runs on blocking threads behind a
//! per-tool timeout — a timeout is an `isError` result with code
//! `timeout`, never a protocol error and never a hang.
//!
//! Modern-only posture lives in two places: the era guard
//! refuses `initialize` before the SDK sees it, and
//! [`ServerHandler::supported_protocol_versions`] pins this
//! handler to 2026-07-28, so `server/discover` advertises
//! exactly one version and the SDK validates every request's
//! `_meta` against it.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities,
    Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::{Value, json};

use crate::TororiState;
use crate::error::TororiError;

/// One tool's static contract.
pub struct ToolSpec {
    pub name: &'static str,
    /// Trigger-scoped loading group (brief §3 groupings).
    pub group: &'static str,
    pub description: &'static str,
    pub schema: Value,
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    /// Documented per-tool execution timeout, seconds.
    pub timeout_secs: u64,
}

/// Access hints: (readOnlyHint, destructiveHint, idempotentHint).
pub const READ: (bool, bool, bool) = (true, false, true);
pub const WRITE: (bool, bool, bool) = (false, false, false);
pub const WRITE_IDEMPOTENT: (bool, bool, bool) = (false, false, true);

fn spec(
    name: &'static str,
    group: &'static str,
    description: &'static str,
    schema: Value,
    (read_only, destructive, idempotent): (bool, bool, bool),
    timeout_secs: u64,
) -> ToolSpec {
    ToolSpec {
        name,
        group,
        description,
        schema,
        read_only,
        destructive,
        idempotent,
        timeout_secs,
    }
}

/// The full surface, in brief §3 order. A tool appears here only
/// when its stage has landed — the surface never advertises a
/// stub.
pub fn tool_specs() -> Vec<ToolSpec> {
    let page_props = json!({
        "offset": {"type": "integer", "minimum": 0},
        "limit": {"type": "integer", "minimum": 1, "maximum": 200}
    });
    vec![
        // ── Server group ──────────────────────────────────────
        spec(
            "torori_status",
            "server",
            "Server version, server-computed catalogue/library/bundle/voice counts, lane states, silence-rule posture.",
            json!({"type": "object", "properties": {}}),
            READ,
            10,
        ),
        // ── Catalogue group ───────────────────────────────────
        spec(
            "torori_catalogue_search",
            "catalogue",
            "Search catalogue entries by text, kind, and status. Bounded, paginated; total is server-computed.",
            json!({"type": "object", "properties": {
                "query": {"type": "string"},
                "kind": {"type": "string", "enum": ["KINK", "ACT", "REACTION"]},
                "status": {"type": "string", "enum": ["HAVE-LIBRARY", "MEASURED-ONLY", "PLANNED", "GAP", "GENERATED"]},
                "offset": page_props["offset"], "limit": page_props["limit"]
            }}),
            READ,
            10,
        ),
        spec(
            "torori_catalogue_get",
            "catalogue",
            "One catalogue entry by slug, in full.",
            json!({"type": "object", "properties": {
                "slug": {"type": "string"}
            }, "required": ["slug"]}),
            READ,
            10,
        ),
        spec(
            "torori_catalogue_stats",
            "catalogue",
            "Server-computed catalogue counts: total, by kind, by status, pair/form coverage.",
            json!({"type": "object", "properties": {}}),
            READ,
            10,
        ),
        spec(
            "torori_catalogue_transition",
            "catalogue",
            "Governed status transition. GENERATED requires asset_refs + §5.2 manifest_refs; leaving GENERATED requires an ear_verdict {verdict, admitted_by} — the Owner's gate event. Provenance class is never overwritten.",
            json!({"type": "object", "properties": {
                "slug": {"type": "string"},
                "to_status": {"type": "string", "enum": ["HAVE-LIBRARY", "MEASURED-ONLY", "PLANNED", "GAP", "GENERATED"]},
                "asset_refs": {"type": "array", "items": {"type": "string"}},
                "manifest_refs": {"type": "array", "items": {"type": "string"}},
                "ear_verdict": {"type": "object", "properties": {
                    "verdict": {"type": "string"},
                    "admitted_by": {"type": "string"},
                    "note": {"type": "string"}
                }, "required": ["verdict", "admitted_by"]}
            }, "required": ["slug", "to_status"]}),
            WRITE,
            10,
        ),
        // ── Library group ─────────────────────────────────────
        spec(
            "torori_library_search",
            "library",
            "Search the OpenNSFW library INDEX by text, pack, and licence state (CLEARED|NOT CLEARED|UNDETERMINED). Bounded, paginated; counts are server-computed.",
            json!({"type": "object", "properties": {
                "query": {"type": "string"},
                "pack": {"type": "string"},
                "license_state": {"type": "string", "enum": ["CLEARED", "NOT CLEARED", "UNDETERMINED"]},
                "offset": page_props["offset"], "limit": page_props["limit"]
            }}),
            READ,
            10,
        ),
        spec(
            "torori_library_fetch",
            "library",
            "Copy ONE CLEARED library file into a project's staging and append its attribution entry in the same operation. UNDETERMINED / NOT CLEARED rows refuse; a drifted file (sha256 mismatch) refuses. No bulk export exists.",
            json!({"type": "object", "properties": {
                "pack": {"type": "string"},
                "file": {"type": "string"},
                "project": {"type": "string"}
            }, "required": ["pack", "file", "project"]}),
            WRITE,
            30,
        ),
        spec(
            "torori_library_manifest",
            "library",
            "A project's attribution manifest: every fetched file with its attribution line, licence, and hashes. Bounded, paginated.",
            json!({"type": "object", "properties": {
                "project": {"type": "string"},
                "offset": page_props["offset"], "limit": page_props["limit"]
            }, "required": ["project"]}),
            READ,
            10,
        ),
        spec(
            "torori_library_ingest",
            "library",
            "Ingest a per-file-capture tree into the INDEX: licence state comes from each file's <file>.meta.json sidecar. No capture → UNDETERMINED, never rounded up; NC/ND → NOT CLEARED. Idempotent per (pack, file).",
            json!({"type": "object", "properties": {
                "pack_dir": {"type": "string"},
                "pack": {"type": "string"}
            }, "required": ["pack_dir", "pack"]}),
            WRITE_IDEMPOTENT,
            300,
        ),
        // ── Bundle + pair group ───────────────────────────────
        spec(
            "torori_bundle_validate",
            "bundles",
            "Validate a cue-bundle directory (bundle.ron + assets) through the full ingest battery — schema, hashes, provenance, tier stamps, target_voice, pairs V1–V8 — without ingesting. First failure in §8 order refuses.",
            json!({"type": "object", "properties": {
                "bundle_dir": {"type": "string"},
                "disclosures": {"type": "array", "items": {"type": "string"}}
            }, "required": ["bundle_dir"]}),
            READ,
            30,
        ),
        spec(
            "torori_bundle_ingest",
            "bundles",
            "Ingest a cue bundle: quarantine-copy, validate (the torori_bundle_validate battery), promote. Idempotent on content hash; partial ingest never happens; unvalidated bundles are never visible to generation tools.",
            json!({"type": "object", "properties": {
                "bundle_dir": {"type": "string"},
                "disclosures": {"type": "array", "items": {"type": "string"}}
            }, "required": ["bundle_dir"]}),
            WRITE_IDEMPOTENT,
            60,
        ),
        spec(
            "torori_bundle_list",
            "bundles",
            "Ingested bundles (id, content hash, producer, cue/pair counts, stored dir). Bounded, paginated.",
            json!({"type": "object", "properties": {
                "offset": page_props["offset"], "limit": page_props["limit"]
            }}),
            READ,
            10,
        ),
        spec(
            "torori_pair_validate",
            "pairs",
            "Run the GAP5 V1–V8 battery over one TriggerPair: completeness, release reachability, precedence + safeword coverage, cue collision, scope, disclosure, ship order. Returns armed_grade + violations.",
            json!({"type": "object", "properties": {
                "pair": {"type": "object"},
                "shared_releases": {"type": "array", "items": {"type": "object"}},
                "asset_index": {"type": "array", "items": {"type": "object", "properties": {
                    "asset_ref": {"type": "string"}, "sha256": {"type": "string"}
                }, "required": ["asset_ref", "sha256"]}},
                "disclosures": {"type": "array", "items": {"type": "string"}}
            }, "required": ["pair"]}),
            READ,
            10,
        ),
        spec(
            "torori_pair_list",
            "pairs",
            "Every pair visible to the server: data/pairs program docs plus ingested bundles. Bounded, paginated.",
            json!({"type": "object", "properties": {
                "offset": page_props["offset"], "limit": page_props["limit"]
            }}),
            READ,
            10,
        ),
        // ── Voice group ───────────────────────────────────────
        spec(
            "torori_voice_list",
            "voice",
            "The VoiceIdentity registry: construction records, locked references, lock states (designed|anchored|ear-admitted). Bounded, paginated.",
            json!({"type": "object", "properties": {
                "offset": page_props["offset"], "limit": page_props["limit"]
            }}),
            READ,
            10,
        ),
        spec(
            "torori_voice_generate",
            "voice",
            "Generate voice takes for one conditioning-v1 record via Qwen3-TTS (v1 slots: induction, fragment, whisper, affirmation). File refs + §5.3 QC metrics only — nothing is played. Each take ships a complete §5.2 manifest (weights sha256 pinned), UNGRADED until the Owner's ear admits it. Unrepresentable conditioning and blocklisted text refuse.",
            json!({"type": "object", "properties": {
                "conditioning": {"type": "object"},
                "slot_class": {"type": "string", "enum": ["induction", "fragment", "whisper", "affirmation"]},
                "takes": {"type": "integer", "minimum": 1, "maximum": 8}
            }, "required": ["conditioning", "slot_class"]}),
            WRITE,
            900,
        ),
        spec(
            "torori_sfx_render",
            "sfx",
            "Render one deterministic DSP instrument (impact | wet_layer | friction | bed | room_tone) to a 48 kHz master WAV with §5.2 manifest. BIT-EXACT only: any other class refuses rather than substituting a lane.",
            json!({"type": "object", "properties": {
                "instrument": {"type": "string", "enum": ["impact", "wet_layer", "friction", "bed", "room_tone"]},
                "params": {"type": "object"},
                "seed": {"type": "integer"},
                "duration_sec": {"type": "number"},
                "determinism_class": {"type": "string", "enum": ["BIT-EXACT", "SEED-REPRODUCIBLE", "OUTPUT-UNIQUE"]}
            }, "required": ["instrument"]}),
            WRITE,
            60,
        ),
        spec(
            "torori_comp_assemble",
            "comp",
            "Route C assembly: base + gained/offset layers (file|generated|library|sfx), deterministic 48 kHz mix, §4 cleanup, §4.3 ship gate. NO-SHIP returns the untreated comp with failing numbers — never silently cleaned. Generated layers into a library base refuse (laundering).",
            json!({"type": "object", "properties": {
                "asset_id": {"type": "string"},
                "base": {"type": "object"},
                "layers": {"type": "array", "items": {"type": "object"}},
                "cleanup": {"type": "boolean"}
            }, "required": ["asset_id", "base"]}),
            WRITE,
            300,
        ),
        spec(
            "torori_voice_qc",
            "voice",
            "Recompute a stored asset's §5.3 QC record, verify §5.2 manifest completeness, report golden deltas when a golden exists.",
            json!({"type": "object", "properties": {
                "asset_id": {"type": "string"}
            }, "required": ["asset_id"]}),
            READ,
            30,
        ),
    ]
}

fn tool_definitions() -> Vec<Tool> {
    tool_specs()
        .into_iter()
        .map(|s| {
            let Value::Object(map) = s.schema else {
                unreachable!("tool schemas are objects");
            };
            let annotations = ToolAnnotations::from_raw(
                None,
                Some(s.read_only),
                Some(s.destructive),
                Some(s.idempotent),
                Some(false),
            );
            Tool::new(s.name, s.description, map).with_annotations(annotations)
        })
        .collect()
}

fn text_result(value: &Value, is_error: bool) -> CallToolResult {
    let content = vec![ContentBlock::text(value.to_string())];
    if is_error {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    }
}

fn ok(value: Value) -> CallToolResult {
    text_result(&value, false)
}

fn err(error: &TororiError) -> CallToolResult {
    let content = vec![ContentBlock::text(error.to_payload())];
    CallToolResult::error(content)
}

/// Dispatch one call to its domain implementation. Returns the
/// success payload, or the typed domain refusal. (Unknown names
/// never reach here from the wire — `call_tool` refuses them as a
/// protocol error first; a direct caller gets `unknown_tool`.)
pub fn dispatch(state: &mut TororiState, name: &str, args: &Value) -> Result<Value, TororiError> {
    match name {
        "torori_status" => Ok(state.status()),
        "torori_catalogue_search" => state.catalogue.search(args),
        "torori_catalogue_get" => state.catalogue.get_tool(args),
        "torori_catalogue_stats" => Ok(state.catalogue.stats()),
        "torori_catalogue_transition" => {
            let out = state.catalogue.transition(args)?;
            state.catalogue.save_overlay(&state.paths)?;
            Ok(out)
        }
        "torori_library_search" => state.library.search(args),
        "torori_library_fetch" => state.library.fetch(&state.paths, args),
        "torori_library_manifest" => state.library.manifest(&state.paths, args),
        "torori_library_ingest" => state.library.ingest(&state.paths, args),
        "torori_bundle_validate" => state
            .bundles
            .validate_tool(&state.paths, &state.voices, args),
        "torori_bundle_ingest" => state.bundles.ingest(&state.paths, &state.voices, args),
        "torori_bundle_list" => state.bundles.list_bundles(args),
        "torori_pair_validate" => state.bundles.pair_validate(args),
        "torori_pair_list" => state.bundles.pair_list(args),
        "torori_voice_list" => state.voices.list(args),
        "torori_voice_generate" => crate::voice::generate(&state.paths, &state.voices, args),
        "torori_voice_qc" => crate::voice::voice_qc(&state.paths, args),
        "torori_sfx_render" => crate::sfx::render(&state.paths, args),
        "torori_comp_assemble" => crate::comp::assemble(&state.paths, &state.library, args),
        _ => Err(TororiError::new(
            "unknown_tool",
            format!("unknown tool: {name}"),
        )),
    }
}

fn render(result: Result<Value, TororiError>) -> CallToolResult {
    match result {
        Ok(value) => ok(value),
        Err(e) => err(&e),
    }
}

pub struct TororiServer {
    state: Arc<Mutex<TororiState>>,
}

impl TororiServer {
    pub fn new(state: TororiState) -> Self {
        TororiServer {
            state: Arc::new(Mutex::new(state)),
        }
    }
}

impl ServerHandler for TororiServer {
    fn get_info(&self) -> InitializeResult {
        let mut server_info = Implementation::default();
        server_info.name = "torori".to_string();
        server_info.version = env!("CARGO_PKG_VERSION").to_string();
        let mut info = InitializeResult::default();
        info.protocol_version = ProtocolVersion::V_2026_07_28;
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = server_info;
        info
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[ProtocolVersion::V_2026_07_28])
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: tool_definitions(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        let Some(spec) = tool_specs().into_iter().find(|s| s.name == name) else {
            return Err(ErrorData::invalid_params(
                format!("unknown tool: {name}"),
                None,
            ));
        };
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let state = Arc::clone(&self.state);
        let call_name = name.clone();
        let work = tokio::task::spawn_blocking(move || {
            let mut guard = state.lock().expect("state lock poisoned");
            render(dispatch(&mut guard, &call_name, &arguments))
        });
        match tokio::time::timeout(std::time::Duration::from_secs(spec.timeout_secs), work).await {
            Ok(Ok(result)) => Ok(result.into()),
            Ok(Err(join)) => Err(ErrorData::internal_error(
                format!("tool task failed: {join}"),
                None,
            )),
            Err(_elapsed) => Ok(CallToolResult::error(vec![ContentBlock::text(
                TororiError::new(
                    "timeout",
                    format!(
                        "tool {name} exceeded its documented {}s timeout",
                        spec.timeout_secs
                    ),
                )
                .to_payload(),
            )])
            .into()),
        }
    }
}
