//! The §5.2 asset manifest: every asset's rights record.
//!
//! A manifest is complete only when every field the §5.2 list
//! names is present and well-formed; `validate` returns the list
//! of problems (empty = complete). Manifests are stored as RON
//! beside their assets and by asset_id in bundle manifests.

use serde::{Deserialize, Serialize};

use crate::error::{TororiError, TororiResult};
use crate::qc::QcRecord;
use crate::util::sha256_hex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeterminismClass {
    #[serde(alias = "BIT-EXACT")]
    BitExact,
    #[serde(alias = "SEED-REPRODUCIBLE")]
    SeedReproducible,
    #[serde(alias = "OUTPUT-UNIQUE")]
    OutputUnique,
}

impl DeterminismClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeterminismClass::BitExact => "BIT-EXACT",
            DeterminismClass::SeedReproducible => "SEED-REPRODUCIBLE",
            DeterminismClass::OutputUnique => "OUTPUT-UNIQUE",
        }
    }

    pub fn parse(s: &str) -> TororiResult<DeterminismClass> {
        match s {
            "BIT-EXACT" => Ok(DeterminismClass::BitExact),
            "SEED-REPRODUCIBLE" => Ok(DeterminismClass::SeedReproducible),
            "OUTPUT-UNIQUE" => Ok(DeterminismClass::OutputUnique),
            other => Err(TororiError::invalid_input(format!(
                "unknown determinism class {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HonestyLabel {
    Measured,
    Authored,
    Inferred,
}

impl HonestyLabel {
    pub fn as_str(&self) -> &'static str {
        match self {
            HonestyLabel::Measured => "MEASURED",
            HonestyLabel::Authored => "AUTHORED",
            HonestyLabel::Inferred => "INFERRED",
        }
    }
}

/// GAP5 §6.2 pair-manifest section, present on pair assets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairSection {
    pub pair_id: String,
    pub cue_sha256: String,
    pub release_form_id: String,
    pub release_asset_sha256s: Vec<String>,
    pub template_refs: Vec<String>,
    /// Pair generation at bundle build time.
    #[serde(rename = "gen")]
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinishRecord {
    pub lufs: f64,
    pub true_peak_dbtp: f64,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupRecord {
    /// Ordered chain: tool names with versions.
    pub chain: Vec<String>,
    /// The §4.3 measurements, as recorded at cleanup time.
    pub gate: Option<crate::comp::GateNumbers>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetManifest {
    pub asset_id: String,
    /// Character id, or the class for class assets.
    pub character_id: String,
    /// Lane name, e.g. "qwen3-tts", "dsp-impact", "route-c-comp".
    pub lane: String,
    /// Model name for model lanes; None for pure DSP lanes.
    pub model_name: Option<String>,
    /// Model weights sha256 — REQUIRED for model lanes.
    pub model_weights_sha256: Option<String>,
    pub code_version: String,
    pub seed: Option<u64>,
    /// Full prompt/instruction text; estate-asset-id provenance
    /// only (never a source performer's name).
    pub prompt: String,
    pub parameters: serde_json::Value,
    pub determinism_class: DeterminismClass,
    pub cleanup: Option<CleanupRecord>,
    pub finish: Option<FinishRecord>,
    pub qc: Option<QcRecord>,
    pub honesty_label: HonestyLabel,
    /// The rights record: licence states, attribution, lane law.
    pub rights: String,
    pub output_sha256: String,
    pub pair: Option<PairSection>,
}

impl AssetManifest {
    pub fn new(asset_id: &str, lane: &str) -> AssetManifest {
        AssetManifest {
            asset_id: asset_id.to_string(),
            character_id: String::new(),
            lane: lane.to_string(),
            model_name: None,
            model_weights_sha256: None,
            code_version: env!("CARGO_PKG_VERSION").to_string(),
            seed: None,
            prompt: String::new(),
            parameters: serde_json::Value::Null,
            determinism_class: DeterminismClass::OutputUnique,
            cleanup: None,
            finish: None,
            qc: None,
            honesty_label: HonestyLabel::Authored,
            rights: String::new(),
            output_sha256: String::new(),
            pair: None,
        }
    }

    /// §5.2 completeness: the problems list (empty = complete).
    pub fn validate(&self) -> Vec<String> {
        let mut problems: Vec<String> = Vec::new();
        let need = |cond: bool, what: &str, problems: &mut Vec<String>| {
            if !cond {
                problems.push(format!("missing/invalid: {what}"));
            }
        };
        need(!self.asset_id.is_empty(), "asset_id", &mut problems);
        need(!self.character_id.is_empty(), "character_id", &mut problems);
        need(!self.lane.is_empty(), "lane", &mut problems);
        need(!self.code_version.is_empty(), "code_version", &mut problems);
        need(!self.prompt.is_empty(), "prompt", &mut problems);
        need(!self.rights.is_empty(), "rights record", &mut problems);
        need(!self.parameters.is_null(), "parameters", &mut problems);
        // Model lanes must pin their weights.
        let is_model_lane = self.model_name.is_some() || self.lane.contains("tts");
        if is_model_lane {
            need(
                self.model_name.as_deref().is_some_and(|s| !s.is_empty()),
                "model_name (model lane)",
                &mut problems,
            );
            need(
                self.model_weights_sha256
                    .as_deref()
                    .is_some_and(is_sha256_hex),
                "model_weights_sha256 (model lane)",
                &mut problems,
            );
        }
        need(
            is_sha256_hex(&self.output_sha256),
            "output_sha256",
            &mut problems,
        );
        need(self.qc.is_some(), "qc record", &mut problems);
        need(self.finish.is_some(), "finish record", &mut problems);
        if let Some(pair) = &self.pair {
            need(!pair.pair_id.is_empty(), "pair.pair_id", &mut problems);
            need(
                is_sha256_hex(&pair.cue_sha256),
                "pair.cue_sha256",
                &mut problems,
            );
            need(
                !pair.release_form_id.is_empty(),
                "pair.release_form_id",
                &mut problems,
            );
            need(
                !pair.release_asset_sha256s.is_empty()
                    && pair.release_asset_sha256s.iter().all(|s| is_sha256_hex(s)),
                "pair.release_asset_sha256s",
                &mut problems,
            );
        }
        problems
    }

    /// Stamp the output hash from the asset's bytes.
    pub fn stamp_output(&mut self, bytes: &[u8]) {
        self.output_sha256 = sha256_hex(bytes);
    }

    pub fn to_ron(&self) -> TororiResult<String> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::new().struct_names(true))
            .map_err(|e| TororiError::new("io", format!("manifest serialize: {e}")))
    }

    pub fn from_ron(text: &str) -> TororiResult<AssetManifest> {
        ron::from_str(text)
            .map_err(|e| TororiError::new("manifest_invalid", format!("manifest parse: {e}")))
    }
}

pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qc::QcRecord;

    fn minimal_qc() -> QcRecord {
        // Built through the real QC path on silence in qc tests;
        // here a literal keeps manifest tests independent.
        serde_json::from_value(serde_json::json!({
            "duration_sec": 1.0, "duration_delta_sec": null,
            "sample_rate": 48000, "channels": 1,
            "noise_floor_dbfs": -60.0, "floor_flatness": 0.5,
            "crest_factor_db": 12.0, "envelope_dynamic_range_db": 6.0,
            "lufs_integrated_est": -16.0, "true_peak_dbtp_est": -1.0,
            "f0_median_hz": null, "f0_range_semitones": null,
            "voiced_fraction": 0.0, "f0_track_stability": null,
            "clipped_samples": 0, "zero_run_max_ms": 0.0,
            "dc_offset": 0.0, "breath_bracket": null,
            "onset_offset_ms": null
        }))
        .expect("qc literal")
    }

    #[test]
    fn completeness_adversarial() {
        let mut m = AssetManifest::new("a1", "dsp-impact");
        assert!(!m.validate().is_empty(), "bare manifest is incomplete");
        m.character_id = "class".into();
        m.prompt = "impact render".into();
        m.parameters = serde_json::json!({"force": 0.5});
        m.rights = "estate-authored DSP; no third-party material".into();
        m.determinism_class = DeterminismClass::BitExact;
        m.output_sha256 = "a".repeat(64);
        m.qc = Some(minimal_qc());
        m.finish = Some(FinishRecord {
            lufs: -16.0,
            true_peak_dbtp: -1.0,
            sample_rate: 48_000,
            channels: 1,
        });
        assert!(m.validate().is_empty(), "problems: {:?}", m.validate());
        // Model lane without weights hash must fail.
        m.lane = "qwen3-tts".into();
        m.model_name = Some("Qwen3-TTS".into());
        assert!(
            m.validate().iter().any(|p| p.contains("model_weights")),
            "weights pin is required for model lanes"
        );
    }
}
