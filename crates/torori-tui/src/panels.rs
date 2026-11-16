//! Pure panel constructors (DESIGN §D2): widget in, buffer out,
//! no I/O, no facade. Every rendered string passes a named
//! truncation bound so an oversized payload cannot widen a panel
//! or panic a render.

use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;
use serde_json::Value;

use crate::app::{Mode, StagedCall};
use crate::facade::CatalogueRow;
use crate::triggers::Rule;

/// Maximum characters of any single rendered field.
pub const FIELD_CHARS_MAX: usize = 120;
/// Maximum rows rendered in the catalogue list panel.
pub const ROWS_RENDER_MAX: usize = 50;
/// Maximum audit lines rendered in the Fleet panel.
pub const AUDIT_RENDER_MAX: usize = 20;

/// Truncate `text` to `max_chars` characters (char-boundary
/// safe), appending an ellipsis marker when truncated.
pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let kept: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{kept}…")
}

fn field(value: &Value, path: &[&str]) -> String {
    let mut current = value;
    for key in path {
        match current.get(key) {
            Some(next) => current = next,
            None => return "—".to_string(),
        }
    }
    match current {
        Value::String(s) => truncate(s, FIELD_CHARS_MAX),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => truncate(&current.to_string(), FIELD_CHARS_MAX),
    }
}

/// Overview: name/version, catalogue size, lane status,
/// voice-lane state, tool count, silence-rule posture.
pub fn overview_panel<'a>(
    status: Option<&'a Value>,
    voice_lane: Option<&'a str>,
    tool_count: usize,
    rule_count: usize,
) -> Paragraph<'a> {
    let mut lines: Vec<Line> = Vec::new();
    match status {
        Some(value) => {
            lines.push(Line::from(format!(
                "{} {}",
                field(value, &["name"]),
                field(value, &["version"])
            )));
            lines.push(Line::from(format!(
                "catalogue entries: {}",
                field(value, &["counts", "catalogue_entries"])
            )));
            lines.push(Line::from(format!(
                "library rows: {}",
                field(value, &["counts", "library_rows"])
            )));
            lines.push(Line::from(format!(
                "bundles ingested: {}",
                field(value, &["counts", "bundles_ingested"])
            )));
            lines.push(Line::from(format!(
                "pairs: {}",
                field(value, &["counts", "pairs"])
            )));
            lines.push(Line::from(format!(
                "voice identities: {}",
                field(value, &["counts", "voice_identities"])
            )));
            lines.push(Line::from(format!(
                "lanes: qwen_tts={} dsp_sfx={} route_c_comp={}",
                field(value, &["lanes", "qwen_tts"]),
                field(value, &["lanes", "dsp_sfx"]),
                field(value, &["lanes", "route_c_comp"]),
            )));
            lines.push(Line::from(format!(
                "silence rule: {}",
                field(value, &["silence_rule"])
            )));
        }
        None => lines.push(Line::from("status: not fetched yet")),
    }
    lines.push(Line::from(format!(
        "voice lane: {}",
        voice_lane.unwrap_or("unknown")
    )));
    lines.push(Line::from(format!("tools: {tool_count}")));
    lines.push(Line::from(format!("trigger rules: {rule_count}")));
    lines.push(Line::from(
        "audio: catalogue rows only — nothing is played or rendered from audio",
    ));
    Paragraph::new(Text::from(lines))
}

/// Workflows: mode, staged call (exact tool + args), and the
/// last catalogue rows (bounded).
pub fn workflows_panel<'a>(
    mode: Mode,
    staged: Option<&'a StagedCall>,
    rows: &'a [CatalogueRow],
    skipped: usize,
) -> Paragraph<'a> {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(format!("mode: {mode:?}")));
    match staged {
        Some(call) => {
            lines.push(Line::from(format!("staged: {}", call.tool)));
            lines.push(Line::from(truncate(
                &call.args.to_string(),
                FIELD_CHARS_MAX * 2,
            )));
        }
        None => lines.push(Line::from("staged: (none)")),
    }
    lines.push(Line::from(
        "provider: n/a — torori routes no model calls (control honestly absent, §D3)",
    ));
    if skipped > 0 {
        lines.push(Line::from(format!("malformed rows skipped: {skipped}")));
    }
    for row in rows.iter().take(ROWS_RENDER_MAX) {
        lines.push(Line::from(format!(
            "{} | {} | {} | {}",
            truncate(&row.slug, 40),
            truncate(&row.title, 40),
            row.kind,
            row.status
        )));
    }
    Paragraph::new(Text::from(lines))
}

/// Log / Live: the bounded event buffer, most recent last.
pub fn log_panel<'a>(log: &'a [String]) -> Paragraph<'a> {
    let lines: Vec<Line> = log
        .iter()
        .map(|line| Line::from(truncate(line, FIELD_CHARS_MAX * 2)))
        .collect();
    Paragraph::new(Text::from(lines))
}

/// Fleet: declared trigger rules with birth stamps + enable
/// state, and the audit tail. Nothing fires that is not listed.
pub fn fleet_panel<'a>(rules: &'a [Rule], audit: &'a [String]) -> Paragraph<'a> {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(
        "fleet: bloch (lesson-candidate sink) — live trigger transport not built; firings are audit-recorded intents",
    ));
    for rule in rules {
        lines.push(Line::from(format!(
            "{} [{} / {}] on {} -> {}.{} mode={} enabled={}",
            rule.id,
            rule.origin_lane,
            rule.content_label,
            rule.on_event,
            rule.target_mcp,
            rule.target_tool,
            rule.mode,
            rule.enabled
        )));
        lines.push(Line::from(format!(
            "  label reason: {}",
            truncate(&rule.content_label_reason, FIELD_CHARS_MAX)
        )));
    }
    lines.push(Line::from("audit:"));
    for line in audit.iter().rev().take(AUDIT_RENDER_MAX).rev() {
        lines.push(Line::from(truncate(line, FIELD_CHARS_MAX * 2)));
    }
    Paragraph::new(Text::from(lines))
}
