//! Cue-table format (TR-2): audio events placed against
//! animation frames, exchanged as a RON file.
//!
//! A cue table is the torori side of the animation chain: an
//! authoring or measurement lane writes one small file naming,
//! for each audio event, the frame it lands on at the table's
//! frame rate — plus, when the event was placed by time, the
//! authored time it came from, so placement error is checkable
//! instead of assumed. No server call is involved; the file is
//! the transport, like the bundle lane.
//!
//! File shape (explicit field names, no silent defaults):
//!
//! ```ron
//! (
//!     schema_version: 1,
//!     table_id: "scene-1-cues",
//!     fps: 60,
//!     events: [
//!         (
//!             event_id: "onset-breath",
//!             frame: 30,
//!             kind: "onset",
//!             slug: Some("reaction-afterglow-recovery"),
//!             asset_ref: None,
//!             at_sec: Some(0.5),
//!         ),
//!     ],
//! )
//! ```
//!
//! The gate (plan torori.md TR-2): an event's frame and its
//! authored time must agree to ≤ 1 frame at the table's rate —
//! `validate` refuses a table where they disagree by more,
//! with the typed code `cue_table_timing`. All work here is on
//! files only; nothing in this module can play audio (the
//! AUDIO SILENCE RULE).

use serde::{Deserialize, Serialize};

use crate::error::{TororiError, TororiResult};

pub const CUE_TABLE_SCHEMA_VERSION: u32 = 1;
/// Bounded result/authoring surface: a cue table is a scene's
/// worth of events, never an unbounded stream.
pub const MAX_CUE_EVENTS: usize = 4096;
pub const MAX_FPS: u32 = 240;
/// The TR-2 gate: placement error budget, in frames.
pub const MAX_FRAME_ERROR: u64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CueTable {
    pub schema_version: u32,
    pub table_id: String,
    /// Animation frame rate the event frames are counted at.
    pub fps: u32,
    /// Events in non-decreasing frame order.
    pub events: Vec<CueTableEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CueTableEvent {
    pub event_id: String,
    /// The animation frame the event lands on.
    pub frame: u64,
    /// Event class as authored (e.g. `onset`, `hit`, `breath`).
    pub kind: String,
    /// Torori catalogue slug, when the event is a catalogue entry.
    pub slug: Option<String>,
    /// Asset file the event sounds, when it names one directly.
    pub asset_ref: Option<String>,
    /// The authored/measured time the frame was placed from,
    /// when known. When present it must agree with `frame` to
    /// within `MAX_FRAME_ERROR` frames (the TR-2 gate).
    pub at_sec: Option<f64>,
}

/// The frame nearest to `sec` at `fps` (round-half-up on the
/// frame grid). Refuses negative or non-finite times.
pub fn frame_at(sec: f64, fps: u32) -> TororiResult<u64> {
    check_fps(fps)?;
    if !sec.is_finite() || sec < 0.0 {
        return Err(TororiError::invalid_input(format!(
            "cue time {sec} is not a finite, non-negative second count"
        )));
    }
    Ok((sec * f64::from(fps)).round() as u64)
}

/// The time of the start of `frame` at `fps`.
pub fn sec_at(frame: u64, fps: u32) -> f64 {
    if fps == 0 {
        return 0.0;
    }
    frame as f64 / f64::from(fps)
}

/// Placement error of an event, in frames: the distance between
/// its stated frame and the frame its authored time lands on.
/// `None` when the event carries no authored time.
pub fn frame_error(event: &CueTableEvent, fps: u32) -> TororiResult<Option<u64>> {
    match event.at_sec {
        Some(sec) => Ok(Some(event.frame.abs_diff(frame_at(sec, fps)?))),
        None => Ok(None),
    }
}

fn check_fps(fps: u32) -> TororiResult<()> {
    if fps == 0 || fps > MAX_FPS {
        return Err(TororiError::new(
            "cue_table_invalid",
            format!("fps {fps} outside 1..={MAX_FPS}"),
        ));
    }
    Ok(())
}

impl CueTable {
    /// Load and validate a cue-table file. A present-but-invalid
    /// table is a hard error with a typed code, never a partial
    /// or silently repaired table.
    pub fn load(path: &std::path::Path) -> TororiResult<CueTable> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| TororiError::new("io", format!("cannot read {}: {e}", path.display())))?;
        let table: CueTable = ron::from_str(&text).map_err(|e| {
            TororiError::new(
                "cue_table_invalid",
                format!("{} does not parse: {e}", path.display()),
            )
        })?;
        table.validate()?;
        Ok(table)
    }

    pub fn validate(&self) -> TororiResult<()> {
        if self.schema_version != CUE_TABLE_SCHEMA_VERSION {
            return Err(TororiError::new(
                "cue_table_invalid",
                format!(
                    "schema_version {} (expected {CUE_TABLE_SCHEMA_VERSION})",
                    self.schema_version
                ),
            ));
        }
        if self.table_id.is_empty() {
            return Err(TororiError::new("cue_table_invalid", "table_id is empty"));
        }
        check_fps(self.fps)?;
        if self.events.len() > MAX_CUE_EVENTS {
            return Err(TororiError::new(
                "cue_table_invalid",
                format!(
                    "{} events exceeds the {MAX_CUE_EVENTS} bound",
                    self.events.len()
                ),
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut previous: Option<u64> = None;
        for event in &self.events {
            if event.event_id.is_empty() || event.kind.is_empty() {
                return Err(TororiError::new(
                    "cue_table_invalid",
                    format!("event {:?} has an empty event_id/kind", event.event_id),
                ));
            }
            if !ids.insert(&event.event_id) {
                return Err(TororiError::new(
                    "cue_table_invalid",
                    format!("duplicate event_id {}", event.event_id),
                ));
            }
            if let Some(prev) = previous
                && event.frame < prev
            {
                return Err(TororiError::new(
                    "cue_table_invalid",
                    format!(
                        "event {} at frame {} follows frame {prev}: events must be in non-decreasing frame order",
                        event.event_id, event.frame
                    ),
                ));
            }
            previous = Some(event.frame);
            if let Some(error) = frame_error(event, self.fps)?
                && error > MAX_FRAME_ERROR
            {
                return Err(TororiError::new(
                    "cue_table_timing",
                    format!(
                        "event {} is placed at frame {} but its authored time lands {} frames away at {} fps (budget: {MAX_FRAME_ERROR})",
                        event.event_id, event.frame, error, self.fps
                    ),
                ));
            }
        }
        Ok(())
    }

    pub fn get(&self, event_id: &str) -> TororiResult<&CueTableEvent> {
        self.events
            .iter()
            .find(|e| e.event_id == event_id)
            .ok_or_else(|| TororiError::not_found(format!("no cue event {event_id}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, frame: u64, at_sec: Option<f64>) -> CueTableEvent {
        CueTableEvent {
            event_id: id.into(),
            frame,
            kind: "onset".into(),
            slug: None,
            asset_ref: None,
            at_sec,
        }
    }

    fn table(events: Vec<CueTableEvent>) -> CueTable {
        CueTable {
            schema_version: CUE_TABLE_SCHEMA_VERSION,
            table_id: "fixture".into(),
            fps: 60,
            events,
        }
    }

    #[test]
    fn frame_conversion_rounds_to_the_nearest_frame() {
        assert_eq!(frame_at(0.5, 60).unwrap(), 30);
        assert_eq!(frame_at(0.0, 60).unwrap(), 0);
        assert_eq!(frame_at(1.0 / 60.0, 60).unwrap(), 1);
        assert_eq!(sec_at(30, 60), 0.5);
        assert!(frame_at(-0.1, 60).is_err());
        assert!(frame_at(f64::NAN, 60).is_err());
        assert!(frame_at(0.5, 0).is_err());
    }

    #[test]
    fn timing_gate_admits_one_frame_and_refuses_two() {
        // 0.5 s at 60 fps is frame 30: frame 31 is within the
        // budget, frame 32 is not.
        table(vec![event("a", 31, Some(0.5))])
            .validate()
            .expect("one frame of error is within budget");
        let out = table(vec![event("a", 32, Some(0.5))]).validate();
        assert_eq!(out.unwrap_err().code, "cue_table_timing");
    }

    #[test]
    fn validation_adversarial_shape_fails() {
        let mut t = table(vec![event("a", 10, None)]);
        t.schema_version = 99;
        assert_eq!(t.validate().unwrap_err().code, "cue_table_invalid");
        let t = table(vec![event("a", 10, None), event("a", 20, None)]);
        assert_eq!(t.validate().unwrap_err().code, "cue_table_invalid");
        let t = table(vec![event("a", 20, None), event("b", 10, None)]);
        assert_eq!(t.validate().unwrap_err().code, "cue_table_invalid");
        let mut t = table(vec![event("a", 10, None)]);
        t.fps = 0;
        assert_eq!(t.validate().unwrap_err().code, "cue_table_invalid");
    }
}
