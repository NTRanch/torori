//! S3 machinery (PLAN §5/§6, D-483): the non-generative batch
//! layer over *existing* deliveries. It assembles a batch
//! manifest from delivered assets and their §5.2 manifests,
//! selects takes by QC metrics (Tier 0 + Tier 1), keeps the
//! refusal bookkeeping, writes a regeneration QUEUE, and emits
//! the subtitle cue sheet (PLAN §5 row shape).
//!
//! **This module never generates.** It contains no driver
//! handle, spawns no process, and consults no driver
//! configuration: the regeneration queue is a data file a
//! future, separately-gated generation step may consume. The
//! `executed: false` on the queue file is not a flag someone
//! forgot to flip — no code path in torori executes a queue.
//!
//! All batching bars it applies (Tier 1 verdicts, the batch
//! mean WER bar) are the PROVISIONAL PLAN §4 values, named as
//! such in every output (D-483).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::asr::{Tier1Record, WerVerdict, normalize_words};
use crate::error::{TororiError, TororiResult};
use crate::manifest::AssetManifest;
use crate::qc::QcRecord;

/// PROVISIONAL (PLAN §4 proposal, D-483): the batch mean WER
/// bar over selected lines. Recorded on every assembly as a
/// within/outside flag; the Owner ratifies the value later.
pub const PROVISIONAL_BATCH_WER_MEAN_MAX: f64 = 0.05;
/// Scene cap, matching the frozen conditioning schema (PLAN
/// §1: scene generation cap 64 lines).
pub const MAX_SCENE_LINES: usize = 64;
/// A delivery directory may hold at most this many takes
/// (64 lines × the lane's 8-take ceiling, plus headroom).
pub const MAX_DELIVERED_TAKES: usize = 512;

/// The Tier 0 hard-gate rule as applied at S1 (recorded in
/// the S1 metrics sheet): manifest QC present, no clipping,
/// true peak below 0 dBTP, no dropout-length zero run,
/// duration inside 1–40 s, voiced fraction at least half.
/// LUFS is deliberately NOT in the rule — the voice lane's
/// stems sit below the dialogue target by design and the
/// −16 LUFS finish is the comp lane's assembly job.
pub fn tier0_pass(qc: &QcRecord) -> bool {
    qc.clipped_samples == 0
        && qc.true_peak_dbtp_est < 0.0
        && qc.zero_run_max_ms < 1000.0
        && (1.0..=40.0).contains(&qc.duration_sec)
        && qc.voiced_fraction >= 0.5
}

/// Why a Tier 0 record fails, for the refusal bookkeeping.
fn tier0_failure_reason(qc: &QcRecord) -> String {
    let mut failed = Vec::new();
    if qc.clipped_samples > 0 {
        failed.push(format!("{} clipped samples", qc.clipped_samples));
    }
    if qc.true_peak_dbtp_est >= 0.0 {
        failed.push(format!("true peak {:.2} dBTP", qc.true_peak_dbtp_est));
    }
    if qc.zero_run_max_ms >= 1000.0 {
        failed.push(format!("zero run {:.0} ms", qc.zero_run_max_ms));
    }
    if !(1.0..=40.0).contains(&qc.duration_sec) {
        failed.push(format!("duration {:.2} s outside 1–40", qc.duration_sec));
    }
    if qc.voiced_fraction < 0.5 {
        failed.push(format!("voiced fraction {:.2}", qc.voiced_fraction));
    }
    format!("tier0 hard gate: {}", failed.join(", "))
}

/// One script line of the scene being assembled. `text` is the
/// authored English line verbatim (D-472); `text_ref` is the
/// reference the cue sheet carries (the content.ron line id in
/// production; the line id itself in the S1 fixture).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneLine {
    pub line_id: String,
    pub text_ref: String,
    pub text: String,
}

/// A caller-supplied timeline position for one line. When a
/// timeline is supplied at all, every selected line must have
/// an entry; `duration_ms`, when present, is an expectation
/// the delivered manifest duration must match exactly
/// (rounded to ms) or the whole assembly refuses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineCue {
    pub line_id: String,
    pub start_ms: u64,
    pub duration_ms: Option<u64>,
}

/// One delivered take loaded from a delivery directory: the
/// §5.2 manifest plus where its WAV sits.
#[derive(Debug, Clone)]
pub struct DeliveredTake {
    pub asset_id: String,
    pub wav_path: PathBuf,
    pub manifest: AssetManifest,
}

/// Load every `<stem>.manifest.ron` + `<stem>.wav` pair in a
/// delivery directory. A manifest that does not parse, fails
/// completeness validation, or lacks its WAV is a typed
/// refusal naming the file — a batch is never assembled over
/// silently skipped evidence. Output is sorted by asset id
/// (deterministic assembly).
pub fn load_delivery_dir(dir: &Path) -> TororiResult<Vec<DeliveredTake>> {
    let mut names: Vec<PathBuf> = Vec::new();
    let entries = std::fs::read_dir(dir)
        .map_err(|e| TororiError::new("io", format!("read delivery dir: {e}")))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| TororiError::new("io", format!("delivery dir entry: {e}")))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".manifest.ron") {
            names.push(entry.path());
        }
    }
    names.sort();
    if names.len() > MAX_DELIVERED_TAKES {
        return Err(TororiError::new(
            "delivery_too_large",
            format!(
                "delivery dir holds {} manifests, above the {MAX_DELIVERED_TAKES} bound",
                names.len()
            ),
        ));
    }
    let mut takes = Vec::new();
    for manifest_path in names {
        let file_name = manifest_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let stem = file_name
            .strip_suffix(".manifest.ron")
            .unwrap_or_default()
            .to_string();
        let wav_path = dir.join(format!("{stem}.wav"));
        if !wav_path.exists() {
            return Err(TororiError::new(
                "delivery_incomplete",
                format!("{file_name}: manifest has no WAV beside it ({stem}.wav missing)"),
            ));
        }
        let text = std::fs::read_to_string(&manifest_path)
            .map_err(|e| TororiError::new("io", format!("read {file_name}: {e}")))?;
        let manifest = AssetManifest::from_ron(&text).map_err(|e| {
            TororiError::new(
                "manifest_invalid",
                format!("{file_name}: does not parse as a §5.2 manifest: {e}"),
            )
        })?;
        let problems = manifest.validate();
        if !problems.is_empty() {
            return Err(TororiError::new(
                "manifest_incomplete",
                format!("{file_name}: {}", problems.join("; ")),
            ));
        }
        takes.push(DeliveredTake {
            asset_id: manifest.asset_id.clone(),
            wav_path,
            manifest,
        });
    }
    takes.sort_by(|a, b| a.asset_id.cmp(&b.asset_id));
    Ok(takes)
}

/// The line text a take was generated from, from its manifest
/// parameters (the conditioning record rides there verbatim).
fn take_text(manifest: &AssetManifest) -> Option<String> {
    manifest
        .parameters
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefusalScope {
    Take,
    Line,
}

/// One bookkeeping entry: a refused take, or a line no take
/// could cover. Reasons name the metric and the (provisional)
/// bar — a refusal is evidence, not a shrug.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Refusal {
    pub scope: RefusalScope,
    pub line_id: String,
    pub asset_id: Option<String>,
    pub reason: String,
}

/// One regeneration-queue entry: a refused line, queued for a
/// future gated generation step. Data only (see module docs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegenEntry {
    pub line_id: String,
    pub source_text: String,
    pub reason: String,
}

/// The selected take for one script line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectedLine {
    pub line_id: String,
    pub text_ref: String,
    pub asset_id: String,
    pub sha256: String,
    pub duration_sec: f64,
    pub tier1_wer: Option<f64>,
}

/// One subtitle cue row (PLAN §5): the cue text is the source
/// line verbatim, keyed by the same line id as the text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CueRow {
    pub line_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text_ref: String,
    pub text: String,
}

/// The assembled batch: the S3 deliverable minus the audio
/// (the audio already exists — this is its production record).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAssembly {
    pub scene_id: String,
    pub identity_ref: String,
    pub selected: Vec<SelectedLine>,
    pub cues: Vec<CueRow>,
    pub refusals: Vec<Refusal>,
    pub regen_queue: Vec<RegenEntry>,
    /// Takes that matched no script line for this identity.
    pub unassigned_takes: Vec<String>,
    /// Mean Tier 1 WER over selected lines that carry a Tier 1
    /// record; `None` when no selected line has one.
    pub batch_mean_wer: Option<f64>,
    /// The mean against `PROVISIONAL_BATCH_WER_MEAN_MAX`.
    pub batch_mean_within_provisional_bar: Option<bool>,
    /// Always true until the Owner ratifies the bars (D-483).
    pub bars_provisional: bool,
}

/// The regeneration queue as written to disk. `executed` is
/// always false: no code path in torori executes a queue
/// (D-483 — generation stays a separately gated act).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegenQueueFile {
    pub scene_id: String,
    pub identity_ref: String,
    pub entries: Vec<RegenEntry>,
    pub executed: bool,
    pub note: String,
}

/// The cue sheet as written to disk (PLAN §5 rows).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CueSheet {
    pub scene_id: String,
    pub rows: Vec<CueRow>,
}

/// Assemble one scene's batch from delivered takes.
///
/// Selection per line, deterministic: a take is refused when
/// its Tier 1 verdict (when a record exists) is Refuse, or
/// its Tier 0 hard gate fails. Survivors rank by verdict
/// (Ship < Flag < none), then lowest WER, then asset id. A
/// line with no survivor is refused as a line and queued for
/// regeneration. Takes refused while a sibling take covers
/// their line are bookkeeping only — no queue entry.
///
/// When `timeline` is supplied, cue starts come from it and
/// any stated duration must match the manifest duration
/// exactly (ms-rounded) or the whole assembly refuses with
/// `cue_duration_mismatch` and nothing is produced. Without a
/// timeline, cues are laid out sequentially in script order
/// with `gap_ms` between lines.
pub fn assemble(
    scene_id: &str,
    identity_ref: &str,
    script: &[SceneLine],
    takes: &[DeliveredTake],
    tier1: &BTreeMap<String, Tier1Record>,
    gap_ms: u64,
    timeline: Option<&[TimelineCue]>,
) -> TororiResult<BatchAssembly> {
    if script.is_empty() || script.len() > MAX_SCENE_LINES {
        return Err(TororiError::invalid_input(format!(
            "a scene batch holds 1..={MAX_SCENE_LINES} lines"
        )));
    }
    let mut seen = std::collections::BTreeSet::new();
    for line in script {
        if !seen.insert(&line.line_id) {
            return Err(TororiError::invalid_input(format!(
                "duplicate line_id {} in scene script",
                line.line_id
            )));
        }
    }
    // Assign takes to lines by normalized text, for this
    // identity only. (Two lines with identical text share the
    // take pool — a recording of the same words serves both;
    // selection is per line and deterministic.)
    let mut pools: Vec<Vec<&DeliveredTake>> = vec![Vec::new(); script.len()];
    let mut unassigned: Vec<String> = Vec::new();
    for take in takes {
        let mut assigned = false;
        if take.manifest.character_id == identity_ref
            && let Some(text) = take_text(&take.manifest)
        {
            let want = normalize_words(&text);
            for (i, line) in script.iter().enumerate() {
                if normalize_words(&line.text) == want {
                    pools[i].push(take);
                    assigned = true;
                }
            }
        }
        if !assigned {
            unassigned.push(take.asset_id.clone());
        }
    }
    unassigned.sort();

    let mut selected: Vec<SelectedLine> = Vec::new();
    let mut refusals: Vec<Refusal> = Vec::new();
    let mut regen_queue: Vec<RegenEntry> = Vec::new();
    let mut durations_ms: BTreeMap<String, u64> = BTreeMap::new();

    for (i, line) in script.iter().enumerate() {
        let pool = &pools[i];
        let mut survivors: Vec<(&DeliveredTake, &QcRecord, Option<&Tier1Record>)> = Vec::new();
        for take in pool {
            let record = tier1.get(&take.asset_id);
            if let Some(record) = record
                && record.verdict == WerVerdict::Refuse
            {
                refusals.push(Refusal {
                    scope: RefusalScope::Take,
                    line_id: line.line_id.clone(),
                    asset_id: Some(take.asset_id.clone()),
                    reason: format!(
                        "tier1 WER {:.3} above the provisional refuse bar {:.2} (bars provisional, D-483)",
                        record.wer,
                        crate::asr::PROVISIONAL_WER_REFUSE_ABOVE
                    ),
                });
                continue;
            }
            match &take.manifest.qc {
                None => refusals.push(Refusal {
                    scope: RefusalScope::Take,
                    line_id: line.line_id.clone(),
                    asset_id: Some(take.asset_id.clone()),
                    reason: "tier0 hard gate: manifest carries no QC record".into(),
                }),
                Some(qc) if !tier0_pass(qc) => refusals.push(Refusal {
                    scope: RefusalScope::Take,
                    line_id: line.line_id.clone(),
                    asset_id: Some(take.asset_id.clone()),
                    reason: tier0_failure_reason(qc),
                }),
                Some(qc) => survivors.push((take, qc, record)),
            }
        }
        survivors.sort_by(|a, b| {
            let rank = |r: Option<&Tier1Record>| match r {
                Some(rec) if rec.verdict == WerVerdict::Ship => 0,
                Some(_) => 1,
                None => 2,
            };
            rank(a.2)
                .cmp(&rank(b.2))
                .then_with(|| {
                    let wa = a.2.map_or(f64::INFINITY, |r| r.wer);
                    let wb = b.2.map_or(f64::INFINITY, |r| r.wer);
                    wa.total_cmp(&wb)
                })
                .then_with(|| a.0.asset_id.cmp(&b.0.asset_id))
        });
        match survivors.first() {
            Some((take, qc, record)) => {
                durations_ms.insert(
                    line.line_id.clone(),
                    (qc.duration_sec * 1000.0).round() as u64,
                );
                selected.push(SelectedLine {
                    line_id: line.line_id.clone(),
                    text_ref: line.text_ref.clone(),
                    asset_id: take.asset_id.clone(),
                    sha256: take.manifest.output_sha256.clone(),
                    duration_sec: qc.duration_sec,
                    tier1_wer: record.map(|r| r.wer),
                });
            }
            None => {
                let reason = if pool.is_empty() {
                    "no delivered take for this line".to_string()
                } else {
                    format!("no selectable take ({} delivered, all refused)", pool.len())
                };
                refusals.push(Refusal {
                    scope: RefusalScope::Line,
                    line_id: line.line_id.clone(),
                    asset_id: None,
                    reason: reason.clone(),
                });
                regen_queue.push(RegenEntry {
                    line_id: line.line_id.clone(),
                    source_text: line.text.clone(),
                    reason,
                });
            }
        }
    }

    let cues = build_cues(script, &selected, &durations_ms, gap_ms, timeline)?;
    let wers: Vec<f64> = selected.iter().filter_map(|s| s.tier1_wer).collect();
    let batch_mean_wer = if wers.is_empty() {
        None
    } else {
        Some((wers.iter().sum::<f64>() / wers.len() as f64 * 1000.0).round() / 1000.0)
    };
    Ok(BatchAssembly {
        scene_id: scene_id.to_string(),
        identity_ref: identity_ref.to_string(),
        selected,
        cues,
        refusals,
        regen_queue,
        unassigned_takes: unassigned,
        batch_mean_wer,
        batch_mean_within_provisional_bar: batch_mean_wer
            .map(|m| m <= PROVISIONAL_BATCH_WER_MEAN_MAX),
        bars_provisional: true,
    })
}

/// Build the cue rows for the selected lines (PLAN §5):
/// timing from the supplied timeline (durations verified) or
/// laid out sequentially with the inter-line gap.
fn build_cues(
    script: &[SceneLine],
    selected: &[SelectedLine],
    durations_ms: &BTreeMap<String, u64>,
    gap_ms: u64,
    timeline: Option<&[TimelineCue]>,
) -> TororiResult<Vec<CueRow>> {
    let by_line: BTreeMap<&str, &SelectedLine> =
        selected.iter().map(|s| (s.line_id.as_str(), s)).collect();
    let mut rows = Vec::new();
    let mut cursor: u64 = 0;
    for line in script {
        let Some(sel) = by_line.get(line.line_id.as_str()) else {
            continue;
        };
        let duration = durations_ms[&line.line_id];
        let start = match timeline {
            Some(entries) => {
                let entry = entries
                    .iter()
                    .find(|e| e.line_id == line.line_id)
                    .ok_or_else(|| {
                        TororiError::invalid_input(format!(
                            "timeline supplied but has no entry for line {}",
                            line.line_id
                        ))
                    })?;
                if let Some(expected) = entry.duration_ms
                    && expected != duration
                {
                    return Err(TororiError::new(
                        "cue_duration_mismatch",
                        format!(
                            "line {}: timeline states {expected} ms, delivered audio measures {duration} ms — cues are derived from delivered manifests, never hand-timed (PLAN §5)",
                            line.line_id
                        ),
                    ));
                }
                entry.start_ms
            }
            None => {
                let start = cursor;
                cursor = start + duration + gap_ms;
                start
            }
        };
        rows.push(CueRow {
            line_id: line.line_id.clone(),
            start_ms: start,
            end_ms: start + duration,
            text_ref: sel.text_ref.clone(),
            text: line.text.clone(),
        });
    }
    Ok(rows)
}

/// Write an assembly to disk as the S3 record set:
/// `batch.ron` (the batch manifest), `cues.ron` (the cue
/// sheet), `regen-queue.ron` (the queue — data only, never
/// executed by any code path in torori).
pub fn write_assembly(dir: &Path, assembly: &BatchAssembly) -> TororiResult<()> {
    std::fs::create_dir_all(dir)
        .map_err(|e| TororiError::new("io", format!("mkdir batch out: {e}")))?;
    let pretty = ron::ser::PrettyConfig::new().struct_names(true);
    let write = |name: &str, text: String| -> TororiResult<()> {
        std::fs::write(dir.join(name), text)
            .map_err(|e| TororiError::new("io", format!("write {name}: {e}")))
    };
    let batch_text = ron::ser::to_string_pretty(assembly, pretty.clone())
        .map_err(|e| TororiError::new("io", format!("batch serialize: {e}")))?;
    write("batch.ron", batch_text)?;
    let sheet = CueSheet {
        scene_id: assembly.scene_id.clone(),
        rows: assembly.cues.clone(),
    };
    let cues_text = ron::ser::to_string_pretty(&sheet, pretty.clone())
        .map_err(|e| TororiError::new("io", format!("cues serialize: {e}")))?;
    write("cues.ron", cues_text)?;
    let queue = RegenQueueFile {
        scene_id: assembly.scene_id.clone(),
        identity_ref: assembly.identity_ref.clone(),
        entries: assembly.regen_queue.clone(),
        executed: false,
        note: "data only — no code path in torori executes this queue; regeneration is a separately gated generation act (D-483)".into(),
    };
    let queue_text = ron::ser::to_string_pretty(&queue, pretty)
        .map_err(|e| TororiError::new("io", format!("queue serialize: {e}")))?;
    write("regen-queue.ron", queue_text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::WavData;
    use crate::manifest::DeterminismClass;

    /// A synthetic delivered take whose embedded QC record is
    /// a real `qc::compute` over a clean 2 s signal (Tier 0
    /// pass) — fabricated through the real QC path, not
    /// hand-tuned. The signal is a syllabically gated tone:
    /// torori's voiced measure counts frames above the noise
    /// floor +6 dB, so a constant tone (which is its own
    /// floor) reads unvoiced; the gate gives speech-like
    /// dynamics, 75% of frames loud.
    fn take(asset_id: &str, identity: &str, text: &str) -> DeliveredTake {
        let samples: Vec<f32> = (0..96_000)
            .map(|i| {
                let t = i as f64 / 48_000.0;
                let gate = if (t * 4.0) % 1.0 < 0.75 { 1.0 } else { 0.004 };
                (0.3 * gate * (2.0 * std::f64::consts::PI * 220.0 * t).sin()) as f32
            })
            .collect();
        let wav = WavData {
            sample_rate: 48_000,
            channels: 1,
            samples,
        };
        let qc = crate::qc::compute(&wav, None, false);
        assert!(tier0_pass(&qc), "fixture tone must pass tier 0");
        let mut manifest = AssetManifest::new(asset_id, "qwen3-tts");
        manifest.character_id = identity.into();
        manifest.model_name = Some("model".into());
        manifest.model_weights_sha256 = Some("a".repeat(64));
        manifest.prompt = format!("text: {text}");
        manifest.parameters = serde_json::json!({"text": text});
        manifest.determinism_class = DeterminismClass::SeedReproducible;
        manifest.rights = "estate AI voice; designed voice".into();
        manifest.output_sha256 = "b".repeat(64);
        manifest.qc = Some(qc);
        manifest.finish = Some(crate::manifest::FinishRecord {
            lufs: -20.0,
            true_peak_dbtp: -4.0,
            sample_rate: 48_000,
            channels: 1,
        });
        DeliveredTake {
            asset_id: asset_id.into(),
            wav_path: PathBuf::from(format!("{asset_id}.wav")),
            manifest,
        }
    }

    fn line(line_id: &str, text: &str) -> SceneLine {
        SceneLine {
            line_id: line_id.into(),
            text_ref: line_id.into(),
            text: text.into(),
        }
    }

    fn tier1(wer: f64) -> Tier1Record {
        Tier1Record {
            model: "Qwen/Qwen3-ASR-0.6B".into(),
            transcript: String::new(),
            wer,
            verdict: WerVerdict::for_wer(wer),
            bars_provisional: true,
        }
    }

    #[test]
    fn selection_prefers_ship_then_lowest_wer() {
        // Validation: of three takes for one line — a Flag at
        // 0.18, a Ship at 0.09, a Ship at 0.03 — the 0.03 Ship
        // wins; ranking is by verdict class before raw WER.
        let script = vec![line("l1", "hello there friend")];
        let takes = vec![
            take("a-flag", "voice.x", "hello there friend"),
            take("a-ship-hi", "voice.x", "hello there friend"),
            take("a-ship-lo", "voice.x", "hello there friend"),
        ];
        let mut t1 = BTreeMap::new();
        t1.insert("a-flag".to_string(), tier1(0.18));
        t1.insert("a-ship-hi".to_string(), tier1(0.09));
        t1.insert("a-ship-lo".to_string(), tier1(0.03));
        let out = assemble("s", "voice.x", &script, &takes, &t1, 300, None).unwrap();
        assert_eq!(out.selected.len(), 1);
        assert_eq!(out.selected[0].asset_id, "a-ship-lo");
        assert_eq!(out.batch_mean_wer, Some(0.03));
        assert_eq!(out.batch_mean_within_provisional_bar, Some(true));
        assert!(out.regen_queue.is_empty());
    }

    #[test]
    fn cue_sheet_sequential_layout() {
        // Validation: two 2 s lines with a 300 ms gap start at
        // 0 and 2300; cue text is the source line verbatim.
        let script = vec![line("l1", "first line"), line("l2", "second line")];
        let takes = vec![
            take("t1", "voice.x", "first line"),
            take("t2", "voice.x", "second line"),
        ];
        let out = assemble("s", "voice.x", &script, &takes, &BTreeMap::new(), 300, None).unwrap();
        assert_eq!(out.cues.len(), 2);
        assert_eq!((out.cues[0].start_ms, out.cues[0].end_ms), (0, 2000));
        assert_eq!((out.cues[1].start_ms, out.cues[1].end_ms), (2300, 4300));
        assert_eq!(out.cues[1].text, "second line");
    }

    #[test]
    fn refused_takes_queue_the_line() {
        // Adversarial: the only take for a line is Tier 1
        // Refuse — the line is refused, queued for
        // regeneration, and gets no cue; the queue file is
        // written with executed=false.
        let script = vec![line("l1", "mumbled beyond recognition")];
        let takes = vec![take("a-bad", "voice.x", "mumbled beyond recognition")];
        let mut t1 = BTreeMap::new();
        t1.insert("a-bad".to_string(), tier1(0.42));
        let out = assemble("s", "voice.x", &script, &takes, &t1, 300, None).unwrap();
        assert!(out.selected.is_empty());
        assert!(out.cues.is_empty());
        assert_eq!(out.regen_queue.len(), 1);
        assert_eq!(out.regen_queue[0].line_id, "l1");
        assert!(
            out.refusals
                .iter()
                .any(|r| r.scope == RefusalScope::Line && r.line_id == "l1")
        );
        let dir = std::env::temp_dir().join(format!("torori-batch-q-{}", std::process::id()));
        write_assembly(&dir, &out).unwrap();
        let queue_text = std::fs::read_to_string(dir.join("regen-queue.ron")).unwrap();
        let queue: RegenQueueFile = ron::from_str(&queue_text).unwrap();
        assert!(!queue.executed, "the queue is data; nothing executes it");
        assert_eq!(queue.entries.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cue_duration_mismatch_refuses_everything() {
        // Adversarial: a caller timeline that disagrees with
        // the delivered duration refuses the whole assembly
        // with the typed code — cues are derived, never
        // hand-timed into place.
        let script = vec![line("l1", "timed line")];
        let takes = vec![take("t1", "voice.x", "timed line")];
        let timeline = vec![TimelineCue {
            line_id: "l1".into(),
            start_ms: 0,
            duration_ms: Some(9_999),
        }];
        let err = assemble(
            "s",
            "voice.x",
            &script,
            &takes,
            &BTreeMap::new(),
            300,
            Some(&timeline),
        )
        .unwrap_err();
        assert_eq!(err.code, "cue_duration_mismatch");
        // A matching duration is accepted and honoured.
        let timeline = vec![TimelineCue {
            line_id: "l1".into(),
            start_ms: 500,
            duration_ms: Some(2_000),
        }];
        let out = assemble(
            "s",
            "voice.x",
            &script,
            &takes,
            &BTreeMap::new(),
            300,
            Some(&timeline),
        )
        .unwrap();
        assert_eq!((out.cues[0].start_ms, out.cues[0].end_ms), (500, 2500));
    }

    #[test]
    fn corrupt_manifest_is_a_typed_refusal() {
        // Adversarial: a delivery dir whose manifest does not
        // parse refuses the load, naming the file — the batch
        // never assembles over skipped evidence.
        let dir = std::env::temp_dir().join(format!("torori-batch-c-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("broken.wav"), b"not audio").unwrap();
        std::fs::write(dir.join("broken.manifest.ron"), b"AssetManifest( oops").unwrap();
        let err = load_delivery_dir(&dir).unwrap_err();
        assert_eq!(err.code, "manifest_invalid");
        assert!(err.message.contains("broken.manifest.ron"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unassigned_and_missing_lines_are_bookkept() {
        // Adversarial: a take for another identity and a
        // script line with no take are both surfaced — the
        // foreign take as unassigned, the line as refused +
        // queued — never silently dropped.
        let script = vec![line("l1", "covered line"), line("l2", "orphan line")];
        let takes = vec![
            take("t1", "voice.x", "covered line"),
            take("t9", "voice.other", "covered line"),
        ];
        let out = assemble("s", "voice.x", &script, &takes, &BTreeMap::new(), 300, None).unwrap();
        assert_eq!(out.selected.len(), 1);
        assert_eq!(out.unassigned_takes, vec!["t9".to_string()]);
        assert_eq!(out.regen_queue.len(), 1);
        assert_eq!(out.regen_queue[0].line_id, "l2");
    }
}
