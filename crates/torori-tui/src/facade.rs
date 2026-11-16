//! The narrow facade the UI programs against (DESIGN §D2).
//!
//! Two implementations: [`LibFacade`] over the torori library
//! crate (the reachable route — torori ships a lib beside its MCP
//! server bin), and [`FakeFacade`], an in-memory scripted double
//! that records every call and is used by every test. Where the
//! library cannot be loaded, [`LibFacade::load`] returns a typed
//! error; the bin reports it and exits — it never substitutes a
//! fake. (An MCP stdio client facade is the documented fallback
//! for server-only members; torori's lib is reachable, so that
//! fallback is not built here.)
//!
//! Every method returns `serde_json::Value`, mirroring the MCP
//! tool payloads one-for-one so the TUI and the server can never
//! drift in shape.

use std::collections::VecDeque;
use std::path::Path;

use serde_json::{Value, json};
use torori::TororiState;
use torori::state::Paths;

/// Typed facade failure. `Malformed` covers server/fake payloads
/// whose shape violates the documented tool contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FacadeError {
    /// The member refused or failed the operation.
    Backend(String),
    /// A payload did not match the documented shape.
    Malformed(String),
    /// Caller input failed validation before any call was made.
    InvalidInput(String),
}

impl std::fmt::Display for FacadeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FacadeError::Backend(m) => write!(f, "backend error: {m}"),
            FacadeError::Malformed(m) => write!(f, "malformed payload: {m}"),
            FacadeError::InvalidInput(m) => write!(f, "invalid input: {m}"),
        }
    }
}

impl std::error::Error for FacadeError {}

/// Result alias for facade operations.
pub type FacadeResult<T> = Result<T, FacadeError>;

/// One parsed catalogue row. Rows are text metadata only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueRow {
    /// Entry slug (unique id).
    pub slug: String,
    /// Display title.
    pub title: String,
    /// KINK | ACT | REACTION.
    pub kind: String,
    /// Provenance / production status string.
    pub status: String,
}

/// Parse catalogue rows out of a search payload. The payload's
/// row array may live under `items` or `entries`, or be the
/// payload itself. A row missing slug/title/kind/status is
/// malformed: it is counted in the returned skip count, never
/// rendered as a half-row and never panics the UI.
pub fn parse_rows(payload: &Value) -> FacadeResult<(Vec<CatalogueRow>, usize)> {
    let nested = payload.get("items").or_else(|| payload.get("entries"));
    let array: &Vec<Value> = match nested {
        Some(value) => value
            .as_array()
            .ok_or_else(|| FacadeError::Malformed("row field is not an array".to_string()))?,
        None => payload
            .as_array()
            .ok_or_else(|| FacadeError::Malformed("search payload has no row array".to_string()))?,
    };
    let mut rows = Vec::new();
    let mut skipped = 0usize;
    for item in array {
        let text = |key: &str| item.get(key).and_then(Value::as_str);
        match (text("slug"), text("title"), text("kind"), text("status")) {
            (Some(slug), Some(title), Some(kind), Some(status)) => rows.push(CatalogueRow {
                slug: slug.to_string(),
                title: title.to_string(),
                kind: kind.to_string(),
                status: status.to_string(),
            }),
            _ => skipped += 1,
        }
    }
    Ok((rows, skipped))
}

/// The operations the torori console needs, nothing wider.
pub trait TororiFacade {
    /// `torori_status` payload.
    fn status(&mut self) -> FacadeResult<Value>;
    /// The member's own MCP tool names, in surface order.
    fn tools(&mut self) -> FacadeResult<Vec<String>>;
    /// `torori_catalogue_search` (bounded page).
    fn catalogue_search(&mut self, query: &str, limit: u64) -> FacadeResult<Value>;
    /// `torori_catalogue_stats`.
    fn catalogue_stats(&mut self) -> FacadeResult<Value>;
    /// `torori_pair_validate` over one pair record.
    fn pair_validate(&mut self, pair: Value) -> FacadeResult<Value>;
    /// `torori_bundle_validate` over one bundle directory.
    fn bundle_validate(&mut self, bundle_dir: &str) -> FacadeResult<Value>;
    /// `torori_bundle_ingest` over one bundle directory.
    fn bundle_ingest(&mut self, bundle_dir: &str) -> FacadeResult<Value>;
    /// `torori_bundle_list` (bounded page).
    fn bundle_list(&mut self) -> FacadeResult<Value>;
    /// `torori_voice_list` (bounded page).
    fn voice_list(&mut self) -> FacadeResult<Value>;
    /// Voice-lane state string (`torori::voice::lane_state`).
    fn voice_lane_state(&mut self) -> FacadeResult<String>;
    /// `torori_voice_generate`. File outputs + metrics only; the
    /// TUI never plays anything this returns.
    fn voice_generate(&mut self, args: Value) -> FacadeResult<Value>;
}

/// The real facade: direct calls into the torori library crate.
pub struct LibFacade {
    state: TororiState,
}

impl LibFacade {
    /// Load torori state from its data/state roots.
    ///
    /// # Errors
    /// Returns [`FacadeError::Backend`] when state load fails
    /// (e.g. a drifted catalogue file — torori refuses loudly).
    pub fn load(paths: Paths) -> FacadeResult<LibFacade> {
        let state = TororiState::load(paths).map_err(|e| FacadeError::Backend(e.to_string()))?;
        Ok(LibFacade { state })
    }

    fn backend<T>(result: torori::error::TororiResult<T>) -> FacadeResult<T> {
        result.map_err(|e| FacadeError::Backend(e.to_string()))
    }
}

impl TororiFacade for LibFacade {
    fn status(&mut self) -> FacadeResult<Value> {
        Ok(self.state.status())
    }

    fn tools(&mut self) -> FacadeResult<Vec<String>> {
        Ok(torori::server::tool_specs()
            .iter()
            .map(|spec| spec.name.to_string())
            .collect())
    }

    fn catalogue_search(&mut self, query: &str, limit: u64) -> FacadeResult<Value> {
        let limit = limit.min(crate::SEARCH_LIMIT_MAX);
        Self::backend(
            self.state
                .catalogue
                .search(&json!({"query": query, "limit": limit, "offset": 0})),
        )
    }

    fn catalogue_stats(&mut self) -> FacadeResult<Value> {
        Ok(self.state.catalogue.stats())
    }

    fn pair_validate(&mut self, pair: Value) -> FacadeResult<Value> {
        Self::backend(self.state.bundles.pair_validate(&json!({"pair": pair})))
    }

    fn bundle_validate(&mut self, bundle_dir: &str) -> FacadeResult<Value> {
        Self::backend(self.state.bundles.validate_tool(
            &self.state.paths,
            &self.state.voices,
            &json!({"bundle_dir": bundle_dir}),
        ))
    }

    fn bundle_ingest(&mut self, bundle_dir: &str) -> FacadeResult<Value> {
        let paths = self.state.paths.clone();
        Self::backend(self.state.bundles.ingest(
            &paths,
            &self.state.voices,
            &json!({"bundle_dir": bundle_dir}),
        ))
    }

    fn bundle_list(&mut self) -> FacadeResult<Value> {
        Self::backend(
            self.state
                .bundles
                .list_bundles(&json!({"limit": 50, "offset": 0})),
        )
    }

    fn voice_list(&mut self) -> FacadeResult<Value> {
        Self::backend(self.state.voices.list(&json!({"limit": 50, "offset": 0})))
    }

    fn voice_lane_state(&mut self) -> FacadeResult<String> {
        Ok(torori::voice::lane_state().to_string())
    }

    fn voice_generate(&mut self, args: Value) -> FacadeResult<Value> {
        Self::backend(torori::voice::generate(
            &self.state.paths,
            &self.state.voices,
            &args,
        ))
    }
}

/// Count files under a bundle directory, stopping at
/// [`crate::BUNDLE_FILES_MAX`]. Used for ingest progress and the
/// oversized-bundle refusal; the walk itself is bounded.
///
/// # Errors
/// Returns [`FacadeError::InvalidInput`] when the path is not a
/// directory, and [`FacadeError::Backend`] on walk failures.
pub fn count_bundle_files(dir: &Path) -> FacadeResult<usize> {
    if !dir.is_dir() {
        return Err(FacadeError::InvalidInput(format!(
            "bundle path {} is not a directory",
            dir.display()
        )));
    }
    let mut count = 0usize;
    let mut stack: Vec<std::path::PathBuf> = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .map_err(|e| FacadeError::Backend(format!("read_dir {}: {e}", current.display())))?;
        for entry in entries {
            let entry = entry.map_err(|e| FacadeError::Backend(format!("bundle walk: {e}")))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                count += 1;
                if count > crate::BUNDLE_FILES_MAX {
                    return Ok(count);
                }
            }
        }
    }
    Ok(count)
}

/// In-memory scripted facade for tests. Records every call as
/// `(method, args-summary)` so tests can prove plan mode made no
/// call, and serves queued responses (or a scripted failure).
pub struct FakeFacade {
    /// Recorded calls, in order.
    pub calls: Vec<(String, String)>,
    /// Queued responses per method name; the last one repeats.
    pub responses: std::collections::HashMap<String, VecDeque<Value>>,
    /// When set, every call fails with this message.
    pub fail_with: Option<String>,
}

impl FakeFacade {
    /// An empty fake; unscripted methods return `{}`.
    pub fn new() -> FakeFacade {
        FakeFacade {
            calls: Vec::new(),
            responses: std::collections::HashMap::new(),
            fail_with: None,
        }
    }

    /// Script the response queue for one method.
    pub fn script(&mut self, method: &str, values: Vec<Value>) {
        self.responses
            .insert(method.to_string(), values.into_iter().collect());
    }

    fn record(&mut self, method: &str, summary: String) -> FacadeResult<Value> {
        self.calls.push((method.to_string(), summary));
        if let Some(message) = &self.fail_with {
            return Err(FacadeError::Backend(message.clone()));
        }
        if let Some(queue) = self.responses.get_mut(method) {
            if queue.len() > 1 {
                if let Some(value) = queue.pop_front() {
                    return Ok(value);
                }
            } else if let Some(value) = queue.front() {
                return Ok(value.clone());
            }
        }
        Ok(json!({}))
    }
}

impl Default for FakeFacade {
    fn default() -> FakeFacade {
        FakeFacade::new()
    }
}

impl TororiFacade for FakeFacade {
    fn status(&mut self) -> FacadeResult<Value> {
        self.record("status", String::new())
    }

    fn tools(&mut self) -> FacadeResult<Vec<String>> {
        let value = self.record("tools", String::new())?;
        let names = value
            .get("tools")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Ok(names)
    }

    fn catalogue_search(&mut self, query: &str, limit: u64) -> FacadeResult<Value> {
        self.record("catalogue_search", format!("{query} limit={limit}"))
    }

    fn catalogue_stats(&mut self) -> FacadeResult<Value> {
        self.record("catalogue_stats", String::new())
    }

    fn pair_validate(&mut self, pair: Value) -> FacadeResult<Value> {
        self.record("pair_validate", pair.to_string())
    }

    fn bundle_validate(&mut self, bundle_dir: &str) -> FacadeResult<Value> {
        self.record("bundle_validate", bundle_dir.to_string())
    }

    fn bundle_ingest(&mut self, bundle_dir: &str) -> FacadeResult<Value> {
        self.record("bundle_ingest", bundle_dir.to_string())
    }

    fn bundle_list(&mut self) -> FacadeResult<Value> {
        self.record("bundle_list", String::new())
    }

    fn voice_list(&mut self) -> FacadeResult<Value> {
        self.record("voice_list", String::new())
    }

    fn voice_lane_state(&mut self) -> FacadeResult<String> {
        let value = self.record("voice_lane_state", String::new())?;
        Ok(value
            .get("lane")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string())
    }

    fn voice_generate(&mut self, args: Value) -> FacadeResult<Value> {
        self.record("voice_generate", args.to_string())
    }
}
