//! PLAN §4 Tier 1 + Tier 2: the analysis tiers over delivered
//! voice assets. Both are *evidence* tiers — neither admits
//! anything; the Owner's ear (Tier 3) is the only admission
//! gate, and nothing in this module touches a lock state.
//!
//! **Tier 1 — ASR round-trip.** A delivered line is transcribed
//! by the pinned ASR model (Qwen3-ASR-0.6B, Apache-2.0 — the
//! estate pin from PLAN §2) through an external driver process
//! configured by `TORORI_ASR_DRIVER` (a command prefix, e.g.
//! "<venv-python> <driver.py>", the `TORORI_QWEN_DRIVER`
//! pattern), and the transcript is scored as word error rate
//! against the source line text. The driver contract is owned
//! here: invoked as `<cmd> --audio <wav>`, it prints one JSON
//! object `{"model": ..., "transcript": ...}` on stdout.
//! Analysis only — transcription of existing files; this module
//! contains no synthesis path of any kind.
//!
//! **The WER bars are PROVISIONAL.** The constants below are
//! the values proposed in PLAN §4; per D-483 they are named
//! provisional in code until the Owner ratifies them. They are
//! not settled bars and no report may present them as such.
//!
//! **Tier 2 — identity-drift alarm.** A `DriftAlarm` rides on
//! the QC record: speaker-similarity (embedding cosine) of a
//! delivered line against the identity's locked anchor.
//! Alarm-only semantics: it flags outliers for the ear pass,
//! it admits nothing and refuses nothing. The computation is
//! currently UNVERIFIED — see `DriftAlarm::assess` for the
//! precise gap; a cosine is never fabricated.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::conditioning::VoiceIdentity;
use crate::error::{TororiError, TororiResult};

/// PROVISIONAL (PLAN §4 proposal, D-483): a line at or below
/// this WER ships on the intelligibility floor.
pub const PROVISIONAL_WER_SHIP_MAX: f64 = 0.10;
/// PROVISIONAL (PLAN §4 proposal, D-483): a line strictly
/// above this WER is flagged refused (regenerate or escalate).
/// Between the two bars a line is flagged for the ear pass.
pub const PROVISIONAL_WER_REFUSE_ABOVE: f64 = 0.20;
/// WER inputs are bounded: word lists longer than this are
/// refused rather than scored (a delivered line is seconds of
/// speech; this is two orders of magnitude above any line).
pub const MAX_WER_WORDS: usize = 4_096;

/// Tier 1 verdict for one line, against the PROVISIONAL bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WerVerdict {
    /// WER ≤ `PROVISIONAL_WER_SHIP_MAX`.
    Ship,
    /// Between the bars: intelligible, flagged for the ear.
    Flag,
    /// WER > `PROVISIONAL_WER_REFUSE_ABOVE`.
    Refuse,
}

impl WerVerdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            WerVerdict::Ship => "ship",
            WerVerdict::Flag => "flag",
            WerVerdict::Refuse => "refuse",
        }
    }

    /// The verdict for one WER value under the provisional bars.
    pub fn for_wer(wer: f64) -> WerVerdict {
        if wer <= PROVISIONAL_WER_SHIP_MAX {
            WerVerdict::Ship
        } else if wer <= PROVISIONAL_WER_REFUSE_ABOVE {
            WerVerdict::Flag
        } else {
            WerVerdict::Refuse
        }
    }
}

/// One Tier 1 round-trip record: what the ASR heard, and how
/// far that is from the source line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tier1Record {
    /// The ASR model that produced the transcript.
    pub model: String,
    pub transcript: String,
    /// Word error rate against the source line (0..1+).
    pub wer: f64,
    pub verdict: WerVerdict,
    /// Always true until the Owner ratifies the bars (D-483).
    pub bars_provisional: bool,
}

/// Normalize text to the word list WER is scored over:
/// lowercased, every non-alphanumeric run a separator,
/// apostrophes inside words dropped ("don't" → "dont") —
/// applied identically to reference and hypothesis.
pub fn normalize_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            current.extend(ch.to_lowercase());
        } else if ch == '\'' || ch == '\u{2019}' {
            // Intra-word apostrophe: dropped, not a separator.
            continue;
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Word-level Levenshtein distance (bounded by MAX_WER_WORDS
/// on each side; callers validate lengths before scoring).
fn word_edit_distance(reference: &[String], hypothesis: &[String]) -> usize {
    let mut prev: Vec<usize> = (0..=hypothesis.len()).collect();
    let mut curr: Vec<usize> = vec![0; hypothesis.len() + 1];
    for (i, ref_word) in reference.iter().enumerate() {
        curr[0] = i + 1;
        for (j, hyp_word) in hypothesis.iter().enumerate() {
            let substitution = if ref_word == hyp_word { 0 } else { 1 };
            curr[j + 1] = (prev[j] + substitution)
                .min(prev[j + 1] + 1)
                .min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[hypothesis.len()]
}

/// Word error rate of `hypothesis` against `reference`:
/// edit distance over normalized words, divided by the
/// reference word count. An empty reference scores 0.0 against
/// an empty hypothesis and 1.0 against any speech.
pub fn wer(reference: &str, hypothesis: &str) -> TororiResult<f64> {
    let reference_words = normalize_words(reference);
    let hypothesis_words = normalize_words(hypothesis);
    if reference_words.len() > MAX_WER_WORDS || hypothesis_words.len() > MAX_WER_WORDS {
        return Err(TororiError::invalid_input(format!(
            "WER inputs are bounded at {MAX_WER_WORDS} words per side"
        )));
    }
    if reference_words.is_empty() {
        return Ok(if hypothesis_words.is_empty() {
            0.0
        } else {
            1.0
        });
    }
    let distance = word_edit_distance(&reference_words, &hypothesis_words);
    Ok(distance as f64 / reference_words.len() as f64)
}

#[derive(Debug, Deserialize)]
struct DriverOutcome {
    model: String,
    transcript: String,
}

/// Run the configured ASR driver over one existing WAV file.
/// `driver_cmd` is the command prefix; `None` (or an empty
/// string) is the typed `asr_unconfigured` refusal — a caller
/// that asked for a measurement the lane cannot make gets a
/// refusal, never a silently skipped tier.
fn transcribe_with(driver_cmd: Option<&str>, wav_path: &Path) -> TororiResult<DriverOutcome> {
    let cmd = driver_cmd.unwrap_or("");
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Err(TororiError::new(
            "asr_unconfigured",
            "the Qwen3-ASR driver is not configured: set TORORI_ASR_DRIVER to the driver command (see tools/qwen_asr_driver.py)",
        ));
    }
    let output = std::process::Command::new(parts[0])
        .args(&parts[1..])
        .arg("--audio")
        .arg(wav_path)
        .output()
        .map_err(|e| TororiError::new("driver_failed", format!("ASR driver spawn: {e}")))?;
    if !output.status.success() {
        return Err(TororiError::new(
            "driver_failed",
            format!(
                "ASR driver exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .last()
                    .unwrap_or("")
            ),
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .ok_or_else(|| TororiError::new("driver_contract", "ASR driver printed no result"))?;
    serde_json::from_str(line).map_err(|e| {
        TororiError::new(
            "driver_contract",
            format!("ASR driver result unparsable: {e}"),
        )
    })
}

/// Tier 1 round-trip for one delivered WAV against its source
/// line, using the driver named by `TORORI_ASR_DRIVER`.
pub fn round_trip(wav_path: &Path, source_text: &str) -> TororiResult<Tier1Record> {
    let cmd = std::env::var("TORORI_ASR_DRIVER").ok();
    round_trip_with(cmd.as_deref(), wav_path, source_text)
}

/// `round_trip` with an explicit driver command (the testable
/// seam; `round_trip` is the env-configured front door).
pub fn round_trip_with(
    driver_cmd: Option<&str>,
    wav_path: &Path,
    source_text: &str,
) -> TororiResult<Tier1Record> {
    if source_text.trim().is_empty() {
        return Err(TororiError::invalid_input(
            "Tier 1 round-trip requires the source line text",
        ));
    }
    let outcome = transcribe_with(driver_cmd, wav_path)?;
    let score = wer(source_text, &outcome.transcript)?;
    Ok(Tier1Record {
        model: outcome.model,
        transcript: outcome.transcript,
        wer: (score * 1000.0).round() / 1000.0,
        verdict: WerVerdict::for_wer(score),
        bars_provisional: true,
    })
}

/// Tier 2 identity-drift alarm (PLAN §4). Alarm-only: this
/// record flags outliers for the Owner's ear pass; it admits
/// nothing, refuses nothing, and never feeds a lock state.
///
/// The cosine is `None` in every record this build produces —
/// the computation is UNVERIFIED, with the gap named on each
/// record (see `assess`). A fabricated score would be worse
/// than an honest absence: the field and the plumbing exist
/// so the day a real embedding extractor lands, the cosine
/// has a home and a consumer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftAlarm {
    /// The identity's locked reference performance, when one
    /// exists; the comparison target the cosine is defined
    /// against.
    pub anchor_ref: Option<String>,
    /// Speaker-embedding cosine vs the anchor. Never
    /// fabricated: `None` until a real extractor exists.
    pub cosine: Option<f64>,
    /// "unverified" | "inert-no-locked-anchor" |
    /// "inert-unknown-identity".
    pub status: String,
    /// The precise reason the cosine is absent (or, one day,
    /// how it was computed).
    pub note: String,
}

impl DriftAlarm {
    /// Assess the alarm for one asset's identity.
    ///
    /// THE GAP, named precisely (two independent halves):
    /// 1. No speaker-embedding extractor exists in the estate
    ///    stack: torori's Rust tree has no embedding code, and
    ///    the Qwen driver contract (`tools/qwen_driver.py`)
    ///    exposes synthesis only — no embedding endpoint.
    ///    (Qwen3-ASR, wired for Tier 1, transcribes; it does
    ///    not emit speaker embeddings either.)
    /// 2. For the only identity in the S1 registry
    ///    (`voice.trial.prima-1`, lock state Designed) there is
    ///    no locked reference to compare against — the anchor
    ///    lock is the Owner's ear act and is tabled (D-481).
    ///
    /// Either half alone leaves the cosine undefined.
    pub fn assess(identity: Option<&VoiceIdentity>) -> DriftAlarm {
        match identity {
            None => DriftAlarm {
                anchor_ref: None,
                cosine: None,
                status: "inert-unknown-identity".into(),
                note: "identity is not in the registry — no anchor exists to compare against"
                    .into(),
            },
            Some(id) => match &id.locked_reference {
                None => DriftAlarm {
                    anchor_ref: None,
                    cosine: None,
                    status: "inert-no-locked-anchor".into(),
                    note: format!(
                        "identity {} is {} with no locked reference — the drift alarm has no anchor to compare against (anchor lock is the Owner's ear act, tabled by D-481)",
                        id.identity_ref,
                        id.lock_state.as_str()
                    ),
                },
                Some(anchor) => DriftAlarm {
                    anchor_ref: Some(anchor.clone()),
                    cosine: None,
                    status: "unverified".into(),
                    note: "UNVERIFIED computation: the estate stack has no speaker-embedding extractor (the Qwen driver contract exposes synthesis only, no embedding endpoint) — cosine is never fabricated".into(),
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wer_scoring_and_normalization() {
        // Validation: identical text, and text differing only
        // in case/punctuation, both score zero.
        assert_eq!(wer("the cat sat", "the cat sat").unwrap(), 0.0);
        assert_eq!(wer("The cat sat.", "the cat sat").unwrap(), 0.0);
        assert_eq!(wer("don't stop", "dont stop").unwrap(), 0.0);
        // One substitution in ten words is exactly the
        // provisional ship bar.
        assert_eq!(
            wer(
                "one two three four five six seven eight nine ten",
                "one two three four five six seven eight nine eleven"
            )
            .unwrap(),
            0.1
        );
        // An insertion counts once.
        assert_eq!(wer("a b c d", "a b x c d").unwrap(), 0.25);
    }

    #[test]
    fn provisional_bar_edges() {
        // Validation + adversarial boundary walk: 0.10 ships,
        // 0.20 still only flags (refusal is strictly above),
        // anything past 0.20 refuses.
        assert_eq!(WerVerdict::for_wer(0.0), WerVerdict::Ship);
        assert_eq!(WerVerdict::for_wer(0.10), WerVerdict::Ship);
        assert_eq!(WerVerdict::for_wer(0.100_000_1), WerVerdict::Flag);
        assert_eq!(WerVerdict::for_wer(0.20), WerVerdict::Flag);
        assert_eq!(WerVerdict::for_wer(0.200_000_1), WerVerdict::Refuse);
        assert_eq!(WerVerdict::for_wer(1.0), WerVerdict::Refuse);
    }

    #[test]
    fn round_trip_refusals_are_typed() {
        // Adversarial: no driver configured is a typed refusal,
        // never a skipped tier or a fabricated record.
        let err = round_trip_with(None, Path::new("x.wav"), "hello world").unwrap_err();
        assert_eq!(err.code, "asr_unconfigured");
        // A driver that exits non-zero is driver_failed.
        let err = round_trip_with(Some("false"), Path::new("x.wav"), "hello world").unwrap_err();
        assert_eq!(err.code, "driver_failed");
        // An empty source line is invalid input, refused
        // before any driver runs.
        let err = round_trip_with(Some("false"), Path::new("x.wav"), "  ").unwrap_err();
        assert_eq!(err.code, "invalid_input");
    }

    #[test]
    fn empty_reference_edge() {
        // Adversarial: empty reference scores 1.0 against any
        // speech and 0.0 against silence; oversize inputs are
        // refused, not scored.
        assert_eq!(wer("", "hello").unwrap(), 1.0);
        assert_eq!(wer("", "").unwrap(), 0.0);
        let huge = vec!["word"; MAX_WER_WORDS + 1].join(" ");
        assert_eq!(wer(&huge, "word").unwrap_err().code, "invalid_input");
    }

    #[test]
    fn drift_alarm_never_fabricates() {
        // Validation/adversarial: every construction path
        // yields cosine None with a named status — the alarm
        // can report its own absence, never a made-up score.
        let unknown = DriftAlarm::assess(None);
        assert_eq!(unknown.status, "inert-unknown-identity");
        assert_eq!(unknown.cosine, None);

        let designed = VoiceIdentity {
            identity_ref: "voice.trial.prima-1".into(),
            construction: crate::conditioning::Construction::VoiceDesign {
                record_ref: "design.trial.prima.1".into(),
                seed_manifest_ref: "seedman.trial.prima.1".into(),
            },
            locked_reference: None,
            accent_profile_ref: None,
            lock_state: crate::conditioning::LockState::Designed,
        };
        let alarm = DriftAlarm::assess(Some(&designed));
        assert_eq!(alarm.status, "inert-no-locked-anchor");
        assert_eq!(alarm.cosine, None);
        assert!(alarm.note.contains("D-481"));

        let mut anchored = designed.clone();
        anchored.locked_reference = Some("assets/anchor.wav".into());
        anchored.lock_state = crate::conditioning::LockState::Anchored;
        let alarm = DriftAlarm::assess(Some(&anchored));
        assert_eq!(alarm.status, "unverified");
        assert_eq!(alarm.cosine, None);
        assert_eq!(alarm.anchor_ref.as_deref(), Some("assets/anchor.wav"));
    }
}
