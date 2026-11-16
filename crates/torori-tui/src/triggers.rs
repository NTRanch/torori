//! The cross-MCP trigger engine (DESIGN §D4), torori's rules.
//!
//! Rules are data (`triggers.toml`), loaded at startup and listed
//! in the Fleet panel. Torori genuinely emits exactly one event
//! kind from its own workflows: `validation.completed`, produced
//! when a pair-validation run finishes. (The often-assumed
//! `cue_bundle.ingested → neemu` rule is NOT shipped: torori emits
//! no such consumable event for neemu, and §D4 forbids rules for
//! events the member does not genuinely produce.)
//!
//! BLOCH lane law (D-1030): every rule carries `origin_lane` and
//! `content_label` from birth. Unclassified NTRanch-origin
//! material defaults to `nsfw` (fail-closed); the `sfw` label is
//! reachable only through the rule whose condition requires the
//! event's material to be the SFW catalogue lane, with the reason
//! recorded on the rule itself.
//!
//! Live-firing status, honestly: evaluation, idempotency, depth
//! caps, and audit are real and fake-tested. The dispatcher seam
//! ([`Dispatcher`]) is where a bloch MCP client call will land;
//! the shipped dispatcher records the firing intent in the audit
//! log and does not place a live call — that transport is a named
//! follow-on rung, never faked as done.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::TRIGGER_DEPTH_HARD_MAX;

/// One bounded predicate condition over a named event field.
/// Comparisons only — no expression language, no eval (§D4).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Condition {
    /// Named event field (e.g. `failure_count`, `material_lane`).
    pub field: String,
    /// One of `eq`, `ne`, `gt`, `ge`, `lt`, `le`.
    pub op: String,
    /// Comparison value, as text; numeric fields compare as i64.
    pub value: String,
}

/// One declared trigger rule.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Rule {
    /// Stable rule id.
    pub id: String,
    /// BLOCH birth stamp: the lane this member lives in.
    pub origin_lane: String,
    /// BLOCH birth stamp: `sfw` or `nsfw`.
    pub content_label: String,
    /// Why this label is correct for this rule's material.
    pub content_label_reason: String,
    /// Event kind this rule listens for.
    pub on_event: String,
    /// Target MCP member name.
    pub target_mcp: String,
    /// Target tool on that member.
    pub target_tool: String,
    /// `auto` or `plan` (§D4 gate semantics).
    pub mode: String,
    /// Chain-depth cap for this rule (hard ceiling 2).
    pub max_depth: u8,
    /// Panel-visible enable state; disabling takes effect live.
    pub enabled: bool,
    /// All conditions must hold (AND).
    #[serde(default)]
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Deserialize)]
struct RulesFile {
    #[serde(default)]
    rule: Vec<Rule>,
}

/// A typed event on torori's own event stream.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerEvent {
    /// Event kind, e.g. `validation.completed`.
    pub kind: String,
    /// Named fields the predicates read.
    pub fields: Vec<(String, String)>,
    /// Chain depth of the workflow that produced this event.
    pub depth: u8,
}

impl TriggerEvent {
    /// Build the `validation.completed` event torori genuinely
    /// emits after a pair-validation run.
    pub fn validation_completed(failure_count: usize, material_lane: &str) -> TriggerEvent {
        TriggerEvent {
            kind: "validation.completed".to_string(),
            fields: vec![
                ("failure_count".to_string(), failure_count.to_string()),
                ("material_lane".to_string(), material_lane.to_string()),
            ],
            depth: 0,
        }
    }

    fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Stable hash of (kind, fields) for idempotency keys.
    fn payload_hash(&self) -> String {
        let mut sorted = self.fields.clone();
        sorted.sort();
        let mut hasher = Sha256::new();
        hasher.update(self.kind.as_bytes());
        for (key, value) in &sorted {
            hasher.update(key.as_bytes());
            hasher.update(b"=");
            hasher.update(value.as_bytes());
            hasher.update(b";");
        }
        format!("{:x}", hasher.finalize())
    }
}

/// A firing intent produced by a matched rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Firing {
    /// Rule that fired.
    pub rule_id: String,
    /// Target member + tool.
    pub target: String,
    /// Tool name on the target.
    pub tool: String,
    /// BLOCH birth stamps carried into the audit + candidate.
    pub origin_lane: String,
    /// BLOCH birth stamps carried into the audit + candidate.
    pub content_label: String,
    /// `auto` or `plan`.
    pub mode: String,
    /// Idempotency key: hash of (rule id, event payload).
    pub idempotency_key: String,
}

/// Where a firing goes. The live bloch client implements this
/// seam in a follow-on rung; tonight's dispatcher audits intent.
pub trait Dispatcher {
    /// Dispatch one firing; returns a human-readable outcome.
    fn dispatch(&mut self, firing: &Firing, event: &TriggerEvent) -> String;
}

/// Dispatcher that records intents without placing any live
/// call. Honest by construction: its outcome string says so.
pub struct AuditOnlyDispatcher {
    /// Every firing it was handed, in order.
    pub dispatched: Vec<Firing>,
}

impl AuditOnlyDispatcher {
    /// A dispatcher with no recorded firings.
    pub fn new() -> AuditOnlyDispatcher {
        AuditOnlyDispatcher {
            dispatched: Vec::new(),
        }
    }
}

impl Default for AuditOnlyDispatcher {
    fn default() -> AuditOnlyDispatcher {
        AuditOnlyDispatcher::new()
    }
}

impl Dispatcher for AuditOnlyDispatcher {
    fn dispatch(&mut self, firing: &Firing, _event: &TriggerEvent) -> String {
        self.dispatched.push(firing.clone());
        "recorded as intent (live bloch transport not built)".to_string()
    }
}

/// The rule engine: rules + idempotency set + bounded audit log.
pub struct TriggerEngine {
    rules: Vec<Rule>,
    fired_keys: HashSet<String>,
    audit: Vec<String>,
}

impl TriggerEngine {
    /// Engine over an explicit rule set (tests, embedded default).
    pub fn new(rules: Vec<Rule>) -> TriggerEngine {
        TriggerEngine {
            rules,
            fired_keys: HashSet::new(),
            audit: Vec::new(),
        }
    }

    /// Load rules from a `triggers.toml` file.
    ///
    /// # Errors
    /// Returns a message string when the file cannot be read or
    /// parsed, or when a rule violates the §D4 invariants
    /// (depth ceiling, known mode, sfw/nsfw label vocabulary).
    pub fn load_file(path: &std::path::Path) -> Result<TriggerEngine, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        TriggerEngine::parse(&text)
    }

    /// Parse + validate rules from TOML text.
    ///
    /// # Errors
    /// As [`TriggerEngine::load_file`].
    pub fn parse(text: &str) -> Result<TriggerEngine, String> {
        let file: RulesFile = toml::from_str(text).map_err(|e| format!("triggers.toml: {e}"))?;
        for rule in &file.rule {
            validate_rule(rule)?;
        }
        Ok(TriggerEngine::new(file.rule))
    }

    /// The shipped default rules (the crate's `triggers.toml`).
    ///
    /// # Errors
    /// Only if the embedded file itself is invalid — a build bug,
    /// surfaced as an error rather than a panic.
    pub fn embedded_default() -> Result<TriggerEngine, String> {
        TriggerEngine::parse(include_str!("../triggers.toml"))
    }

    /// All rules, for the Fleet panel.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Enable/disable a rule by id, effective immediately (§D4).
    /// Returns false when the id is unknown.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        if let Some(rule) = self.rules.iter_mut().find(|rule| rule.id == id) {
            rule.enabled = enabled;
            true
        } else {
            false
        }
    }

    /// The bounded audit tail (JSON lines, oldest first).
    pub fn audit(&self) -> &[String] {
        &self.audit
    }

    fn push_audit(&mut self, line: String) {
        if self.audit.len() >= crate::AUDIT_LINES_MAX {
            self.audit.remove(0);
        }
        self.audit.push(line);
    }

    fn audit_line(&mut self, rule: &Rule, event: &TriggerEvent, outcome: &str) {
        let line = json!({
            "rule": rule.id,
            "event": event.kind,
            "origin_lane": rule.origin_lane,
            "content_label": rule.content_label,
            "target": format!("{}.{}", rule.target_mcp, rule.target_tool),
            "outcome": outcome,
        })
        .to_string();
        self.push_audit(line);
    }

    /// Evaluate one event against every rule. Matched rules fire
    /// through `dispatcher`; duplicates (same rule + payload) are
    /// dropped and audited; events beyond a rule's depth cap are
    /// refused and audited. Returns the firings that dispatched.
    pub fn evaluate(
        &mut self,
        event: &TriggerEvent,
        dispatcher: &mut dyn Dispatcher,
    ) -> Vec<Firing> {
        let mut fired = Vec::new();
        // Rules are cloned up front so audit/dispatch can borrow
        // the engine mutably while iterating.
        for rule in self.rules.clone() {
            if !rule.enabled || rule.on_event != event.kind {
                continue;
            }
            if !conditions_hold(&rule.conditions, event) {
                continue;
            }
            if event.depth >= rule.max_depth.min(TRIGGER_DEPTH_HARD_MAX) {
                self.audit_line(&rule, event, "refused: depth cap");
                continue;
            }
            let key = format!("{}:{}", rule.id, event.payload_hash());
            if self.fired_keys.contains(&key) {
                self.audit_line(&rule, event, "dropped: duplicate (idempotency)");
                continue;
            }
            let firing = Firing {
                rule_id: rule.id.clone(),
                target: rule.target_mcp.clone(),
                tool: rule.target_tool.clone(),
                origin_lane: rule.origin_lane.clone(),
                content_label: rule.content_label.clone(),
                mode: rule.mode.clone(),
                idempotency_key: key.clone(),
            };
            self.fired_keys.insert(key);
            let outcome = dispatcher.dispatch(&firing, event);
            self.audit_line(&rule, event, &format!("fired: {outcome}"));
            fired.push(firing);
        }
        fired
    }
}

fn validate_rule(rule: &Rule) -> Result<(), String> {
    if rule.max_depth > TRIGGER_DEPTH_HARD_MAX {
        return Err(format!(
            "rule {}: max_depth {} exceeds hard ceiling {TRIGGER_DEPTH_HARD_MAX}",
            rule.id, rule.max_depth
        ));
    }
    if rule.mode != "auto" && rule.mode != "plan" {
        return Err(format!("rule {}: unknown mode {:?}", rule.id, rule.mode));
    }
    if rule.content_label != "sfw" && rule.content_label != "nsfw" {
        return Err(format!(
            "rule {}: content_label must be sfw|nsfw, got {:?}",
            rule.id, rule.content_label
        ));
    }
    if rule.origin_lane.is_empty() || rule.content_label_reason.is_empty() {
        return Err(format!(
            "rule {}: origin_lane and content_label_reason are required birth stamps",
            rule.id
        ));
    }
    Ok(())
}

fn conditions_hold(conditions: &[Condition], event: &TriggerEvent) -> bool {
    conditions.iter().all(|condition| {
        let Some(actual) = event.field(&condition.field) else {
            return false;
        };
        match condition.op.as_str() {
            "eq" => actual == condition.value,
            "ne" => actual != condition.value,
            "gt" | "ge" | "lt" | "le" => {
                let (Ok(a), Ok(b)) = (actual.parse::<i64>(), condition.value.parse::<i64>()) else {
                    return false;
                };
                match condition.op.as_str() {
                    "gt" => a > b,
                    "ge" => a >= b,
                    "lt" => a < b,
                    _ => a <= b,
                }
            }
            _ => false,
        }
    })
}

/// Build the lesson-candidate argument payload for a firing:
/// the validation summary, stamped with the rule's birth fields.
pub fn candidate_args(firing: &Firing, event: &TriggerEvent) -> Value {
    json!({
        "origin_lane": firing.origin_lane,
        "content_label": firing.content_label,
        "source_event": event.kind,
        "failure_count": event.field("failure_count").unwrap_or("0"),
        "material_lane": event.field("material_lane").unwrap_or("unclassified"),
        "rule": firing.rule_id,
        "draft": "pair validation completed with failures; see torori audit log",
    })
}
