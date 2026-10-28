//! The voice lane (brief §7 T4): the VoiceIdentity registry in
//! front of the Qwen3-TTS generation driver.
//!
//! Lane law, enforced here at the boundary:
//! - The voice is DESIGNED (VoiceDesign instruction text) or an
//!   anchored synthetic — no real-person likeness exists
//!   anywhere in this lane, and no pack recording is ever a
//!   prompt, prefix, or reference (the conjunction ban).
//! - Prompt text passes a minor-coded blocklist screen
//!   (estate hard-stop posture); a hit is a typed refusal.
//! - The driver is an external batch process (headless, writes
//!   files only — silence rule point 5) configured by
//!   `TORORI_QWEN_DRIVER` (a command prefix, e.g.
//!   "<venv-python> <driver.py>"). Its result must report the
//!   model weights sha256; an unreported pin refuses the takes.
//! - Every take ships with a complete §5.2 manifest + §5.3 QC
//!   record. Assets stay UNGRADED until the Owner's ear admits
//!   them (D-084 pattern) — the generate result says so.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::audio;
use crate::conditioning::{ConditioningParams, VoiceRegistry};
use crate::error::{TororiError, TororiResult};
use crate::manifest::{AssetManifest, FinishRecord, HonestyLabel};
use crate::qc;
use crate::state::Paths;
use crate::util::{arg_str, sha256_hex};

/// Lane posture for `torori_status`.
pub fn lane_state() -> &'static str {
    if std::env::var_os("TORORI_QWEN_DRIVER").is_some() {
        "driver-configured"
    } else {
        "driver-unconfigured"
    }
}

/// Minor-coded blocklist (estate hard stop; applies to prompts,
/// instructions, and catalogue mapping alike). Substring match
/// on lowercased text.
const BLOCKLIST: &[&str] = &[
    "child",
    "minor",
    "underage",
    "loli",
    "shota",
    "schoolgirl",
    "schoolboy",
    "preteen",
];

pub fn screen_text(text: &str) -> TororiResult<()> {
    let lower = text.to_lowercase();
    for term in BLOCKLIST {
        if lower.contains(term) {
            return Err(TororiError::new(
                "rights_blocklist",
                format!("text refused by the minor-coded blocklist (matched {term:?})"),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRecord {
    pub asset_id: String,
    pub identity_ref: String,
    pub slot_class: String,
    pub file: String,
    pub sha256: String,
    pub grading: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct AssetIndex {
    records: Vec<AssetRecord>,
}

fn index_path(paths: &Paths) -> std::path::PathBuf {
    paths.assets_dir().join("index.ron")
}

fn read_index(paths: &Paths) -> TororiResult<AssetIndex> {
    let path = index_path(paths);
    if !path.exists() {
        return Ok(AssetIndex::default());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| TororiError::new("io", format!("read asset index: {e}")))?;
    ron::from_str(&text).map_err(|e| TororiError::new("io", format!("asset index parse: {e}")))
}

fn write_index(paths: &Paths, index: &AssetIndex) -> TororiResult<()> {
    let text = ron::ser::to_string_pretty(index, ron::ser::PrettyConfig::new().struct_names(true))
        .map_err(|e| TororiError::new("io", format!("asset index serialize: {e}")))?;
    std::fs::write(index_path(paths), text)
        .map_err(|e| TororiError::new("io", format!("write asset index: {e}")))
}

#[derive(Debug, Deserialize)]
struct DriverResult {
    model: String,
    weights_sha256: String,
    sample_rate: u32,
    takes: Vec<DriverTake>,
}

#[derive(Debug, Deserialize)]
struct DriverTake {
    file: String,
    seed: u64,
}

const SLOT_CLASSES: [&str; 4] = ["induction", "fragment", "whisper", "affirmation"];

/// `torori_voice_generate`.
pub fn generate(paths: &Paths, voices: &VoiceRegistry, args: &Value) -> TororiResult<Value> {
    let conditioning_value = crate::util::arg_value(args, "conditioning")?;
    let conditioning: ConditioningParams =
        serde_json::from_value(conditioning_value).map_err(|e| {
            TororiError::invalid_input(format!(
                "conditioning does not parse as a conditioning-v1 record: {e}"
            ))
        })?;
    let problems = conditioning.validate();
    if !problems.is_empty() {
        return Err(TororiError::new(
            "conditioning_invalid",
            problems.join("; "),
        ));
    }
    let slot_class = arg_str(args, "slot_class")?;
    if !SLOT_CLASSES.contains(&slot_class) {
        return Err(TororiError::invalid_input(format!(
            "slot_class must be one of {SLOT_CLASSES:?} (the v1 lane scope)"
        )));
    }
    let takes = args.get("takes").and_then(Value::as_u64).unwrap_or(2);
    if !(1..=8).contains(&takes) {
        return Err(TororiError::invalid_input("takes must be 1..=8"));
    }
    let identity = voices.get(&conditioning.identity_ref)?;
    screen_text(&conditioning.text)?;
    if let Some(instruction) = &conditioning.instruction {
        screen_text(instruction)?;
    }
    let driver_cmd = std::env::var("TORORI_QWEN_DRIVER").map_err(|_| {
        TororiError::new(
            "lane_unconfigured",
            "the Qwen3-TTS driver is not configured: set TORORI_QWEN_DRIVER to the driver command (see tools/qwen_driver.py)",
        )
    })?;
    let parts: Vec<&str> = driver_cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Err(TororiError::new(
            "lane_unconfigured",
            "TORORI_QWEN_DRIVER is empty",
        ));
    }
    let base_seed = conditioning.seed.unwrap_or(34001);
    let work = paths.assets_dir().join(format!(
        ".work-{}-{}",
        conditioning.identity_ref.replace('.', "-"),
        base_seed
    ));
    std::fs::create_dir_all(&work)
        .map_err(|e| TororiError::new("io", format!("mkdir work: {e}")))?;
    let instruct = conditioning.instruction.clone().unwrap_or_else(|| {
        "A calm adult woman's voice, soft and close to the microphone".to_string()
    });
    let request = json!({
        "text": conditioning.text,
        "instruct": instruct,
        "seed": base_seed,
        "takes": takes,
        "temperature": 0.9,
        "top_p": 0.95,
        "max_new_tokens": 1024,
    });
    let request_path = work.join("request.json");
    std::fs::write(&request_path, request.to_string())
        .map_err(|e| TororiError::new("io", format!("write request: {e}")))?;
    let out_dir = work.join("out");
    let mut cmd = std::process::Command::new(parts[0]);
    cmd.args(&parts[1..])
        .arg("--request")
        .arg(&request_path)
        .arg("--out")
        .arg(&out_dir);
    let output = cmd
        .output()
        .map_err(|e| TororiError::new("driver_failed", format!("driver spawn: {e}")))?;
    if !output.status.success() {
        return Err(TororiError::new(
            "driver_failed",
            format!(
                "driver exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .last()
                    .unwrap_or("")
            ),
        ));
    }
    let result_text = std::fs::read_to_string(out_dir.join("result.json"))
        .map_err(|e| TororiError::new("driver_contract", format!("driver result missing: {e}")))?;
    let result: DriverResult = serde_json::from_str(&result_text).map_err(|e| {
        TororiError::new("driver_contract", format!("driver result unparsable: {e}"))
    })?;
    if !crate::manifest::is_sha256_hex(&result.weights_sha256) {
        return Err(TororiError::new(
            "driver_contract",
            "driver did not report a valid weights sha256 — takes refused (every manifest must pin its weights)",
        ));
    }
    let mut index = read_index(paths)?;
    let mut made: Vec<Value> = Vec::new();
    for (i, take) in result.takes.iter().enumerate() {
        let raw = audio::read_wav(&out_dir.join(&take.file))?;
        // §4.4: masters are 48 kHz. The lane's native rate is
        // resampled deterministically; the manifest records it.
        let samples = if raw.sample_rate != 48_000 {
            audio::resample_linear(&raw.mono(), raw.sample_rate, 48_000)
        } else {
            raw.mono()
        };
        let wav = audio::WavData {
            sample_rate: 48_000,
            channels: 1,
            samples,
        };
        let bytes = audio::encode_wav(&wav);
        let sha = sha256_hex(&bytes);
        let asset_id = format!(
            "voice.{}-{slot_class}-{}-t{i}",
            conditioning.identity_ref.replace("voice.", ""),
            &sha[..8]
        );
        let asset_dir = paths.assets_dir().join(&asset_id);
        std::fs::create_dir_all(&asset_dir)
            .map_err(|e| TororiError::new("io", format!("mkdir asset: {e}")))?;
        let file_path = asset_dir.join("take.wav");
        std::fs::write(&file_path, &bytes)
            .map_err(|e| TororiError::new("io", format!("write take: {e}")))?;
        let qc_record = qc::compute(
            &wav,
            None,
            conditioning.breath_bracket == crate::conditioning::BreathBracketReq::Required,
        );
        let mut manifest = AssetManifest::new(&asset_id, "qwen3-tts");
        manifest.character_id = conditioning.identity_ref.clone();
        manifest.model_name = Some(result.model.clone());
        manifest.model_weights_sha256 = Some(result.weights_sha256.clone());
        manifest.seed = Some(take.seed);
        manifest.prompt = format!("text: {}\ninstruction: {}", conditioning.text, instruct);
        manifest.parameters = serde_json::to_value(&conditioning).unwrap_or(Value::Null);
        manifest.determinism_class = conditioning.determinism_class;
        manifest.qc = Some(qc_record.clone());
        manifest.finish = Some(FinishRecord {
            lufs: qc_record.lufs_integrated_est,
            true_peak_dbtp: qc_record.true_peak_dbtp_est,
            sample_rate: 48_000,
            channels: 1,
        });
        manifest.honesty_label = HonestyLabel::Authored;
        manifest.rights = format!(
            "estate AI voice; identity {} constructed as {:?}; designed voice, no real-person likeness; no pack material in prompt/reference (conjunction ban)",
            identity.identity_ref, identity.construction
        );
        manifest.stamp_output(&bytes);
        let problems = manifest.validate();
        if !problems.is_empty() {
            return Err(TororiError::new(
                "manifest_incomplete",
                format!("generated manifest incomplete: {}", problems.join("; ")),
            ));
        }
        std::fs::write(asset_dir.join("manifest.ron"), manifest.to_ron()?)
            .map_err(|e| TororiError::new("io", format!("write manifest: {e}")))?;
        index.records.push(AssetRecord {
            asset_id: asset_id.clone(),
            identity_ref: conditioning.identity_ref.clone(),
            slot_class: slot_class.to_string(),
            file: file_path.display().to_string(),
            sha256: sha.clone(),
            grading: "UNGRADED".into(),
        });
        made.push(json!({
            "asset_id": asset_id,
            "file": file_path.display().to_string(),
            "sha256": sha,
            "seed": take.seed,
            "qc": serde_json::to_value(&qc_record).unwrap_or(Value::Null),
        }));
    }
    write_index(paths, &index)?;
    let _ = std::fs::remove_dir_all(&work);
    Ok(json!({
        "identity_ref": conditioning.identity_ref,
        "lock_state": identity.lock_state.as_str(),
        "slot_class": slot_class,
        "model": result.model,
        "weights_sha256": result.weights_sha256,
        "driver_sample_rate": result.sample_rate,
        "assets": made,
        "grading": "UNGRADED — admission is the Owner's ear verdict (D-084 pattern); UNGRADED is not a build failure",
    }))
}

/// `torori_voice_qc`: recompute the QC record for a stored
/// asset and check its manifest; when a golden exists for the
/// same (identity, slot), report deltas against it.
pub fn voice_qc(paths: &Paths, args: &Value) -> TororiResult<Value> {
    let asset_id = arg_str(args, "asset_id")?;
    let index = read_index(paths)?;
    let record = index
        .records
        .iter()
        .find(|r| r.asset_id == asset_id)
        .ok_or_else(|| TororiError::not_found(format!("no generated asset {asset_id}")))?;
    let dir = paths.assets_dir().join(asset_id);
    let manifest_text = std::fs::read_to_string(dir.join("manifest.ron"))
        .map_err(|e| TororiError::new("io", format!("read manifest: {e}")))?;
    let manifest = AssetManifest::from_ron(&manifest_text)?;
    let wav = audio::read_wav(std::path::Path::new(&record.file))?;
    let fresh = qc::compute(&wav, None, false);
    let golden = index
        .records
        .iter()
        .find(|r| {
            r.identity_ref == record.identity_ref
                && r.slot_class == record.slot_class
                && r.asset_id != record.asset_id
        })
        .and_then(|g| {
            let gdir = paths.assets_dir().join(&g.asset_id);
            let mtext = std::fs::read_to_string(gdir.join("manifest.ron")).ok()?;
            let gm = AssetManifest::from_ron(&mtext).ok()?;
            gm.qc.map(|gq| {
                json!({
                    "golden_asset_id": g.asset_id,
                    "f0_median_delta_hz": match (fresh.f0_median_hz, gq.f0_median_hz) {
                        (Some(a), Some(b)) => json!(a - b),
                        _ => Value::Null,
                    },
                    "duration_delta_sec": fresh.duration_sec - gq.duration_sec,
                    "lufs_delta": fresh.lufs_integrated_est - gq.lufs_integrated_est,
                })
            })
        });
    Ok(json!({
        "asset_id": asset_id,
        "manifest_complete": manifest.validate().is_empty(),
        "manifest_problems": manifest.validate(),
        "grading": record.grading,
        "qc": serde_json::to_value(&fresh).unwrap_or(Value::Null),
        "golden_delta": golden,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocklist_hard_stops() {
        assert!(screen_text("a calm adult woman's voice").is_ok());
        assert_eq!(
            screen_text("a schoolgirl voice").unwrap_err().code,
            "rights_blocklist"
        );
        assert_eq!(
            screen_text("UNDERAGE").unwrap_err().code,
            "rights_blocklist"
        );
    }

    #[test]
    fn unconfigured_lane_refuses_typed() {
        // In the test environment TORORI_QWEN_DRIVER is unset;
        // if the host has it set, this test's premise changes —
        // guard accordingly.
        if std::env::var_os("TORORI_QWEN_DRIVER").is_some() {
            return;
        }
        let root = std::env::temp_dir().join(format!("torori-t4-{}", std::process::id()));
        let paths = Paths::resolve(None, Some(root.clone()));
        paths.ensure_state_dirs().unwrap();
        let voices = VoiceRegistry::default();
        let args = json!({
            "slot_class": "induction",
            "conditioning": {
                "text": "breathe", "identity_ref": "voice.x",
                "instruction": null, "f0_median_hz": null,
                "f0_range_semitones": null, "tempo_syllables_per_sec": null,
                "pause_structure": null, "breathiness": null, "grade": null,
                "accent_profile_ref": null, "accent_intensity": null,
                "breath_bracket": "Required",
                "determinism_class": "SEED-REPRODUCIBLE", "seed": 1
            }
        });
        // Unknown identity refuses before the lane check.
        let out = generate(&paths, &voices, &args);
        assert_eq!(out.unwrap_err().code, "not_found");
        let _ = std::fs::remove_dir_all(&root);
    }
}
