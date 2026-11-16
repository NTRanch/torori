//! App state machine + the shared execute path (DESIGN §D2).
//!
//! Both front ends (full-screen ratatui and non-TTY line mode)
//! drive this one state machine, so a workflow behaves identical
//! in either. Plan mode stages a call — exact tool + args — and
//! makes **no** facade call until approval; auto mode executes
//! declared operations directly. Every run appends typed events
//! to the bounded log and feeds genuine events to the trigger
//! engine.

use serde_json::{Value, json};

use crate::facade::{FacadeError, TororiFacade, count_bundle_files, parse_rows};
use crate::triggers::{AuditOnlyDispatcher, TriggerEngine, TriggerEvent};

/// The four screens. No Media screen: torori's catalogue carries
/// no image artifacts (see crate docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Name/version, catalogue size, lane status, voice state.
    Overview,
    /// Catalogue browse/search, pair validation, ingest, voice.
    Workflows,
    /// Bounded scrolling event view.
    Log,
    /// Trigger rules + audit tail + fleet reachability note.
    Fleet,
}

/// Run control: plan (stage + approve) or auto (execute).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Stage the call; nothing runs before approval.
    Plan,
    /// Execute declared operations directly.
    Auto,
}

/// One selectable workflow on the Workflows screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Workflow {
    /// Browse/search the catalogue by text.
    CatalogueSearch {
        /// Free-text query.
        query: String,
    },
    /// Run the GAP5 V1–V8 pair-validation battery over one pair.
    PairValidate {
        /// The pair record, as JSON text (parsed at staging).
        pair_json: String,
        /// Material lane of the validated material:
        /// `sfw-catalogue` or anything else (fail-closed nsfw).
        material_lane: String,
    },
    /// Ingest one cue-bundle directory, with live progress.
    BundleIngest {
        /// Bundle directory path.
        dir: String,
    },
    /// Stage a Qwen3-TTS voice-lane job (file outputs only).
    VoiceJob {
        /// Conditioning record, as JSON text.
        conditioning_json: String,
        /// induction | fragment | whisper | affirmation.
        slot_class: String,
        /// Takes, 1..=VOICE_TAKES_MAX.
        takes: u64,
    },
}

impl Workflow {
    /// The MCP tool this workflow calls when executed.
    pub fn tool_name(&self) -> &'static str {
        match self {
            Workflow::CatalogueSearch { .. } => "torori_catalogue_search",
            Workflow::PairValidate { .. } => "torori_pair_validate",
            Workflow::BundleIngest { .. } => "torori_bundle_ingest",
            Workflow::VoiceJob { .. } => "torori_voice_generate",
        }
    }

    /// Build the exact call arguments for staging. Parsing and
    /// bounds are validated here, so a staged call is always a
    /// well-formed one.
    ///
    /// # Errors
    /// Returns [`FacadeError::InvalidInput`] on unparsable JSON,
    /// an unknown slot class, or an out-of-bounds take count.
    pub fn build_args(&self) -> Result<Value, FacadeError> {
        match self {
            Workflow::CatalogueSearch { query } => Ok(json!({"query": query})),
            Workflow::PairValidate { pair_json, .. } => {
                let pair: Value = serde_json::from_str(pair_json).map_err(|e| {
                    FacadeError::InvalidInput(format!("pair JSON does not parse: {e}"))
                })?;
                Ok(json!({"pair": pair}))
            }
            Workflow::BundleIngest { dir } => Ok(json!({"bundle_dir": dir})),
            Workflow::VoiceJob {
                conditioning_json,
                slot_class,
                takes,
            } => {
                let conditioning: Value = serde_json::from_str(conditioning_json).map_err(|e| {
                    FacadeError::InvalidInput(format!("conditioning JSON does not parse: {e}"))
                })?;
                if !matches!(
                    slot_class.as_str(),
                    "induction" | "fragment" | "whisper" | "affirmation"
                ) {
                    return Err(FacadeError::InvalidInput(format!(
                        "unknown slot_class {slot_class:?}"
                    )));
                }
                if *takes == 0 || *takes > crate::VOICE_TAKES_MAX {
                    return Err(FacadeError::InvalidInput(format!(
                        "takes must be 1..={}, got {takes}",
                        crate::VOICE_TAKES_MAX
                    )));
                }
                Ok(json!({
                    "conditioning": conditioning,
                    "slot_class": slot_class,
                    "takes": takes,
                }))
            }
        }
    }
}

/// A staged call awaiting approval (plan mode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedCall {
    /// Workflow it came from.
    pub workflow: Workflow,
    /// MCP tool name.
    pub tool: String,
    /// Exact arguments that will be sent on approval.
    pub args: Value,
}

/// Typed run events; the same stream the Log screen renders and
/// the trigger engine consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    /// A run started.
    Started(String),
    /// Progress: done of total units, with a label.
    Progress {
        /// Units completed.
        done: usize,
        /// Units total.
        total: usize,
        /// What the units are.
        label: String,
    },
    /// A run finished with a payload.
    Done(Value),
    /// A run failed with a message.
    Failed(String),
    /// A plain log line.
    Log(String),
}

/// The console state machine.
pub struct App {
    /// Facade the workflows run against.
    pub facade: Box<dyn TororiFacade>,
    /// Trigger engine (Fleet panel + validation events).
    pub engine: TriggerEngine,
    /// Dispatcher handed to the engine.
    pub dispatcher: AuditOnlyDispatcher,
    /// Current screen.
    pub screen: Screen,
    /// Plan or auto.
    pub mode: Mode,
    /// Staged call awaiting approval, if any.
    pub staged: Option<StagedCall>,
    /// Bounded event log (oldest first).
    pub log: Vec<String>,
    /// Last catalogue rows rendered on Workflows.
    pub last_rows: Vec<crate::facade::CatalogueRow>,
    /// Rows skipped as malformed in the last search.
    pub last_skipped: usize,
    /// Last `torori_status` payload, once fetched.
    pub status: Option<Value>,
    /// Voice-lane state string, once fetched.
    pub voice_lane: Option<String>,
    /// MCP tool count, once fetched.
    pub tool_count: usize,
    /// Set by the `quit` line command / q key.
    pub should_quit: bool,
}

impl App {
    /// New app in plan mode on the Overview screen. Plan is the
    /// default because staging is meaningful for every member
    /// (§D3); auto is an explicit operator choice.
    pub fn new(facade: Box<dyn TororiFacade>, engine: TriggerEngine) -> App {
        App {
            facade,
            engine,
            dispatcher: AuditOnlyDispatcher::new(),
            screen: Screen::Overview,
            mode: Mode::Plan,
            staged: None,
            log: Vec::new(),
            last_rows: Vec::new(),
            last_skipped: 0,
            status: None,
            voice_lane: None,
            tool_count: 0,
            should_quit: false,
        }
    }

    /// Append one bounded log line.
    pub fn log_line(&mut self, line: String) {
        if self.log.len() >= crate::LOG_LINES_MAX {
            self.log.remove(0);
        }
        self.log.push(line);
    }

    /// Refresh Overview data (status + voice lane).
    pub fn refresh_overview(&mut self) {
        match self.facade.status() {
            Ok(value) => {
                self.status = Some(value);
                self.log_line("status refreshed".to_string());
            }
            Err(e) => self.log_line(format!("status failed: {e}")),
        }
        match self.facade.voice_lane_state() {
            Ok(lane) => self.voice_lane = Some(lane),
            Err(e) => self.log_line(format!("voice lane failed: {e}")),
        }
        match self.facade.tools() {
            Ok(names) => self.tool_count = names.len(),
            Err(e) => self.log_line(format!("tools failed: {e}")),
        }
    }

    /// Stage a workflow for approval. Makes no facade call.
    ///
    /// # Errors
    /// Propagates argument-build failures; nothing is staged then.
    pub fn stage(&mut self, workflow: Workflow) -> Result<StagedCall, FacadeError> {
        let args = workflow.build_args()?;
        let staged = StagedCall {
            tool: workflow.tool_name().to_string(),
            workflow,
            args,
        };
        self.staged = Some(staged.clone());
        self.log_line(format!(
            "staged {} {} (plan mode — no call made)",
            staged.tool, staged.args
        ));
        Ok(staged)
    }

    /// Discard the staged call without executing it.
    pub fn abort_staged(&mut self) {
        if let Some(staged) = self.staged.take() {
            self.log_line(format!("aborted staged {}", staged.tool));
        }
    }

    /// Approve and execute the staged call, if any.
    pub fn approve_staged(&mut self) -> Vec<RunEvent> {
        let Some(staged) = self.staged.take() else {
            return vec![RunEvent::Log("nothing staged".to_string())];
        };
        self.execute_workflow(&staged.workflow)
    }

    /// Run a workflow under the current mode: plan stages, auto
    /// executes. Returns the run's events (also logged).
    pub fn run(&mut self, workflow: Workflow) -> Vec<RunEvent> {
        match self.mode {
            Mode::Plan => match self.stage(workflow) {
                Ok(staged) => vec![RunEvent::Log(format!(
                    "staged {} — approve to run",
                    staged.tool
                ))],
                Err(e) => vec![RunEvent::Failed(e.to_string())],
            },
            Mode::Auto => self.execute_workflow(&workflow),
        }
    }

    fn emit(&mut self, events: &mut Vec<RunEvent>, event: RunEvent) {
        let line = match &event {
            RunEvent::Started(what) => format!("started: {what}"),
            RunEvent::Progress { done, total, label } => {
                format!("progress: {done}/{total} {label}")
            }
            RunEvent::Done(_) => "done".to_string(),
            RunEvent::Failed(message) => format!("failed: {message}"),
            RunEvent::Log(line) => line.clone(),
        };
        self.log_line(line);
        events.push(event);
    }

    fn execute_workflow(&mut self, workflow: &Workflow) -> Vec<RunEvent> {
        let mut events = Vec::new();
        self.emit(
            &mut events,
            RunEvent::Started(workflow.tool_name().to_string()),
        );
        let result = match workflow {
            Workflow::CatalogueSearch { query } => self.execute_search(query, &mut events),
            Workflow::PairValidate {
                pair_json,
                material_lane,
            } => self.execute_pair_validate(pair_json, material_lane, &mut events),
            Workflow::BundleIngest { dir } => self.execute_ingest(dir, &mut events),
            Workflow::VoiceJob { .. } => match workflow.build_args() {
                Ok(args) => self.facade.voice_generate(args),
                Err(e) => Err(e),
            },
        };
        match result {
            Ok(value) => self.emit(&mut events, RunEvent::Done(value)),
            Err(e) => self.emit(&mut events, RunEvent::Failed(e.to_string())),
        }
        events
    }

    fn execute_search(
        &mut self,
        query: &str,
        events: &mut Vec<RunEvent>,
    ) -> Result<Value, FacadeError> {
        let payload = self
            .facade
            .catalogue_search(query, crate::SEARCH_LIMIT_MAX)?;
        let (rows, skipped) = parse_rows(&payload)?;
        self.last_skipped = skipped;
        if skipped > 0 {
            self.emit(
                events,
                RunEvent::Log(format!("skipped {skipped} malformed catalogue row(s)")),
            );
        }
        self.last_rows = rows;
        Ok(payload)
    }

    fn execute_pair_validate(
        &mut self,
        pair_json: &str,
        material_lane: &str,
        events: &mut Vec<RunEvent>,
    ) -> Result<Value, FacadeError> {
        let args = Workflow::PairValidate {
            pair_json: pair_json.to_string(),
            material_lane: material_lane.to_string(),
        }
        .build_args()?;
        let pair = args
            .get("pair")
            .cloned()
            .ok_or_else(|| FacadeError::Malformed("staged args lost the pair".to_string()))?;
        let result = self.facade.pair_validate(pair)?;
        let failure_count = result
            .get("violations")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        // Torori genuinely emits this event; the engine decides
        // whether any declared rule fires on it.
        let event = TriggerEvent::validation_completed(failure_count, material_lane);
        let fired = self.engine.evaluate(&event, &mut self.dispatcher);
        for firing in &fired {
            self.emit(
                events,
                RunEvent::Log(format!(
                    "trigger fired: {} -> {}.{} [{} / {}]",
                    firing.rule_id,
                    firing.target,
                    firing.tool,
                    firing.origin_lane,
                    firing.content_label
                )),
            );
        }
        Ok(result)
    }

    fn execute_ingest(
        &mut self,
        dir: &str,
        events: &mut Vec<RunEvent>,
    ) -> Result<Value, FacadeError> {
        let path = std::path::Path::new(dir);
        let files = count_bundle_files(path)?;
        if files > crate::BUNDLE_FILES_MAX {
            return Err(FacadeError::InvalidInput(format!(
                "bundle has more than {} files (bounded ingest refusal)",
                crate::BUNDLE_FILES_MAX
            )));
        }
        self.emit(
            events,
            RunEvent::Progress {
                done: 1,
                total: 3,
                label: format!("counted {files} bundle file(s)"),
            },
        );
        let validated = self.facade.bundle_validate(dir)?;
        self.emit(
            events,
            RunEvent::Progress {
                done: 2,
                total: 3,
                label: "bundle validated".to_string(),
            },
        );
        let _ = validated;
        let result = self.facade.bundle_ingest(dir)?;
        self.emit(
            events,
            RunEvent::Progress {
                done: 3,
                total: 3,
                label: "bundle ingested".to_string(),
            },
        );
        Ok(result)
    }

    /// One line-mode command. Shares the execute path with the
    /// full-screen UI; returns the lines to print.
    pub fn handle_line(&mut self, line: &str) -> Vec<String> {
        let line = line.trim();
        if line.len() > crate::INPUT_LINE_BYTES_MAX {
            return vec!["error: input line too long".to_string()];
        }
        let mut parts = line.splitn(2, ' ');
        let command = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();
        match command {
            "" => Vec::new(),
            "quit" | "exit" => {
                self.should_quit = true;
                vec!["bye".to_string()]
            }
            "help" => vec![
                "commands: status | tools | search <query> | stats | pair <pair-json> [lane] | ingest <dir> | voice <slot> <takes> <conditioning-json> | mode plan|auto | approve | abort | triggers | screen overview|workflows|log|fleet | quit"
                    .to_string(),
            ],
            "status" => {
                self.refresh_overview();
                vec![match &self.status {
                    Some(value) => value.to_string(),
                    None => "status unavailable".to_string(),
                }]
            }
            "tools" => match self.facade.tools() {
                Ok(names) => vec![names.join(", ")],
                Err(e) => vec![format!("error: {e}")],
            },
            "search" => self.line_run(Workflow::CatalogueSearch {
                query: rest.to_string(),
            }),
            "stats" => match self.facade.catalogue_stats() {
                Ok(value) => vec![value.to_string()],
                Err(e) => vec![format!("error: {e}")],
            },
            "pair" => {
                let (json_text, lane) = split_lane(rest);
                self.line_run(Workflow::PairValidate {
                    pair_json: json_text.to_string(),
                    material_lane: lane.to_string(),
                })
            }
            "ingest" => self.line_run(Workflow::BundleIngest {
                dir: rest.to_string(),
            }),
            "voice" => match parse_voice(rest) {
                Ok(workflow) => self.line_run(workflow),
                Err(e) => vec![format!("error: {e}")],
            },
            "mode" => {
                self.mode = if rest == "auto" { Mode::Auto } else { Mode::Plan };
                vec![format!("mode: {:?}", self.mode)]
            }
            "approve" => summarise(self.approve_staged()),
            "abort" => {
                self.abort_staged();
                vec!["aborted".to_string()]
            }
            "triggers" => self
                .engine
                .rules()
                .iter()
                .map(|rule| {
                    format!(
                        "{} [{} / {}] on {} -> {}.{} ({}) enabled={}",
                        rule.id,
                        rule.origin_lane,
                        rule.content_label,
                        rule.on_event,
                        rule.target_mcp,
                        rule.target_tool,
                        rule.mode,
                        rule.enabled
                    )
                })
                .collect(),
            "screen" => {
                self.screen = match rest {
                    "workflows" => Screen::Workflows,
                    "log" => Screen::Log,
                    "fleet" => Screen::Fleet,
                    _ => Screen::Overview,
                };
                vec![format!("screen: {:?}", self.screen)]
            }
            other => vec![format!("error: unknown command {other:?} (try: help)")],
        }
    }

    fn line_run(&mut self, workflow: Workflow) -> Vec<String> {
        summarise(self.run(workflow))
    }
}

/// Split a trailing ` lane=<lane>` suffix off a pair command.
fn split_lane(rest: &str) -> (&str, &str) {
    if let Some(idx) = rest.rfind(" lane=") {
        (&rest[..idx], &rest[idx + 6..])
    } else {
        (rest, "unclassified")
    }
}

fn parse_voice(rest: &str) -> Result<Workflow, FacadeError> {
    let mut parts = rest.splitn(3, ' ');
    let slot = parts.next().unwrap_or("");
    let takes: u64 = parts
        .next()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| FacadeError::InvalidInput("voice: takes missing".to_string()))?;
    let conditioning = parts.next().unwrap_or("");
    if conditioning.is_empty() {
        return Err(FacadeError::InvalidInput(
            "voice: conditioning JSON missing".to_string(),
        ));
    }
    Ok(Workflow::VoiceJob {
        conditioning_json: conditioning.to_string(),
        slot_class: slot.to_string(),
        takes,
    })
}

fn summarise(events: Vec<RunEvent>) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            RunEvent::Started(what) => format!("started: {what}"),
            RunEvent::Progress { done, total, label } => {
                format!("progress: {done}/{total} {label}")
            }
            RunEvent::Done(value) => format!("done: {value}"),
            RunEvent::Failed(message) => format!("failed: {message}"),
            RunEvent::Log(line) => line.clone(),
        })
        .collect()
}
