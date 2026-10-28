//! The conditioning-parameter schema (STUDY §6.2 / brief §6)
//! and the VoiceIdentity registry.
//!
//! **SCHEMA FREEZE (T3):** `CONDITIONING_SCHEMA_VERSION = 1`.
//! The field set below is the contract neemu S3's `delivery`
//! normalization targets. Changes from here are version bumps,
//! never silent edits. The machine-readable descriptor lives at
//! `data/schemas/conditioning-v1.json`, embedded below and
//! asserted in sync with this module by test.
//!
//! Validation law (brief §6): torori validates ranges and
//! **refuses unrepresentable targets rather than clamping** — a
//! delivery target outside a field's range is a typed refusal at
//! the boundary, never a silently adjusted generation.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{TororiError, TororiResult};
use crate::manifest::DeterminismClass;
use crate::state::Paths;

pub const CONDITIONING_SCHEMA_VERSION: u32 = 1;
pub const CONDITIONING_SCHEMA_JSON: &str = include_str!("../data/schemas/conditioning-v1.json");

/// One conditioning record: how a line is to be voiced. Mirrors
/// the brief §6 field list exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditioningParams {
    pub text: String,
    /// Estate VoiceIdentity ref — never a source performer.
    pub identity_ref: String,
    pub instruction: Option<String>,
    pub f0_median_hz: Option<f64>,
    pub f0_range_semitones: Option<f64>,
    pub tempo_syllables_per_sec: Option<f64>,
    /// e.g. "single terminal fall; breath at clause joins".
    pub pause_structure: Option<String>,
    /// 0..1 (AC conditioning field).
    pub breathiness: Option<f64>,
    /// Arousal grade 0..1 (AC-2).
    pub grade: Option<f64>,
    pub accent_profile_ref: Option<String>,
    /// 0..1; required iff `accent_profile_ref` is present.
    pub accent_intensity: Option<f64>,
    /// `required` for fragments (AC-3): the breath bracket is
    /// part of the asset.
    pub breath_bracket: BreathBracketReq,
    pub determinism_class: DeterminismClass,
    /// Seed policy: an explicit seed, or null = lane-assigned
    /// (the assigned seed is recorded in the manifest).
    pub seed: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreathBracketReq {
    Required,
    NotRequired,
}

impl ConditioningParams {
    /// Range validation. Returns the problems list (empty =
    /// representable). Callers refuse on any problem.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut need = |cond: bool, what: &str| {
            if !cond {
                problems.push(what.to_string());
            }
        };
        need(!self.text.is_empty(), "text is required");
        need(!self.identity_ref.is_empty(), "identity_ref is required");
        if let Some(f0) = self.f0_median_hz {
            need(
                (40.0..=600.0).contains(&f0),
                "f0_median_hz out of range 40..=600",
            );
        }
        if let Some(r) = self.f0_range_semitones {
            need(
                (0.0..=36.0).contains(&r),
                "f0_range_semitones out of range 0..=36",
            );
        }
        if let Some(t) = self.tempo_syllables_per_sec {
            need(
                (0.5..=12.0).contains(&t),
                "tempo_syllables_per_sec out of range 0.5..=12",
            );
        }
        if let Some(b) = self.breathiness {
            need((0.0..=1.0).contains(&b), "breathiness out of range 0..=1");
        }
        if let Some(g) = self.grade {
            need((0.0..=1.0).contains(&g), "grade out of range 0..=1");
        }
        match (&self.accent_profile_ref, &self.accent_intensity) {
            (Some(_), None) => {
                problems.push("accent_intensity is required when accent_profile_ref is set".into())
            }
            (None, Some(_)) => problems
                .push("accent_intensity without accent_profile_ref is unrepresentable".into()),
            (Some(_), Some(i)) => need(
                (0.0..=1.0).contains(i),
                "accent_intensity out of range 0..=1",
            ),
            (None, None) => {}
        }
        problems
    }
}

/// How a VoiceIdentity was constructed (AC-9: no identity
/// without a construction record).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Construction {
    /// A VoiceDesign record + the seed manifest that produced
    /// the reference performance.
    VoiceDesign {
        record_ref: String,
        seed_manifest_ref: String,
    },
    /// A fully synthetic identity, anchored by construction.
    AnchoredSynthetic { notes: String },
}

/// Lock state ordering: designed < anchored < ear-admitted.
/// The last transition is the Owner's verdict, full stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LockState {
    Designed,
    Anchored,
    EarAdmitted,
}

impl LockState {
    pub fn as_str(&self) -> &'static str {
        match self {
            LockState::Designed => "designed",
            LockState::Anchored => "anchored",
            LockState::EarAdmitted => "ear-admitted",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceIdentity {
    pub identity_ref: String,
    pub construction: Construction,
    /// The one locked reference performance (the Aria rule).
    /// Changing it is a re-lock event with its own gate.
    pub locked_reference: Option<String>,
    pub accent_profile_ref: Option<String>,
    pub lock_state: LockState,
}

#[derive(Debug, Serialize, Deserialize)]
struct RegistryFile {
    identities: Vec<VoiceIdentity>,
}

#[derive(Debug, Default)]
pub struct VoiceRegistry {
    pub identities: Vec<VoiceIdentity>,
}

impl VoiceRegistry {
    pub fn load(paths: &Paths) -> TororiResult<VoiceRegistry> {
        let file = paths.voices_file();
        if !file.exists() {
            return Ok(VoiceRegistry::default());
        }
        let text = std::fs::read_to_string(&file)
            .map_err(|e| TororiError::new("io", format!("cannot read {}: {e}", file.display())))?;
        let parsed: RegistryFile = ron::from_str(&text).map_err(|e| {
            TororiError::new("voices_invalid", format!("voice registry parse: {e}"))
        })?;
        let registry = VoiceRegistry {
            identities: parsed.identities,
        };
        registry.validate()?;
        Ok(registry)
    }

    /// Registry invariants: unique refs; an identity at
    /// `anchored` or better must carry a locked reference.
    pub fn validate(&self) -> TororiResult<()> {
        let mut seen = std::collections::BTreeSet::new();
        for id in &self.identities {
            if !seen.insert(&id.identity_ref) {
                return Err(TororiError::new(
                    "voices_invalid",
                    format!("duplicate identity_ref {}", id.identity_ref),
                ));
            }
            if id.lock_state >= LockState::Anchored && id.locked_reference.is_none() {
                return Err(TororiError::new(
                    "voices_invalid",
                    format!(
                        "identity {} is {} without a locked reference",
                        id.identity_ref,
                        id.lock_state.as_str()
                    ),
                ));
            }
        }
        Ok(())
    }

    pub fn get(&self, identity_ref: &str) -> TororiResult<&VoiceIdentity> {
        self.identities
            .iter()
            .find(|i| i.identity_ref == identity_ref)
            .ok_or_else(|| TororiError::not_found(format!("no VoiceIdentity {identity_ref}")))
    }

    /// Bundle-ingest resolution (brief §8): a cue's
    /// `target_voice` must resolve to an estate identity in
    /// `anchored` state or better.
    pub fn resolve_anchored(&self, identity_ref: &str) -> TororiResult<&VoiceIdentity> {
        let id = self.get(identity_ref)?;
        if id.lock_state < LockState::Anchored {
            return Err(TororiError::new(
                "voice_not_anchored",
                format!(
                    "VoiceIdentity {identity_ref} is {} — bundles may only target anchored or ear-admitted identities",
                    id.lock_state.as_str()
                ),
            ));
        }
        Ok(id)
    }

    pub fn to_json(&self, id: &VoiceIdentity) -> Value {
        json!({
            "identity_ref": id.identity_ref,
            "construction": match &id.construction {
                Construction::VoiceDesign { record_ref, seed_manifest_ref } => json!({
                    "kind": "voice-design",
                    "record_ref": record_ref,
                    "seed_manifest_ref": seed_manifest_ref,
                }),
                Construction::AnchoredSynthetic { notes } => json!({
                    "kind": "anchored-synthetic",
                    "notes": notes,
                }),
            },
            "locked_reference": id.locked_reference,
            "accent_profile_ref": id.accent_profile_ref,
            "lock_state": id.lock_state.as_str(),
        })
    }

    pub fn list(&self, args: &Value) -> TororiResult<Value> {
        let page = crate::util::Page::parse(args)?;
        let items: Vec<Value> = self.identities.iter().map(|i| self.to_json(i)).collect();
        Ok(page.apply(&items))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> ConditioningParams {
        ConditioningParams {
            text: "breathe with me".into(),
            identity_ref: "voice.fixture".into(),
            instruction: None,
            f0_median_hz: Some(210.0),
            f0_range_semitones: Some(4.0),
            tempo_syllables_per_sec: Some(3.2),
            pause_structure: None,
            breathiness: Some(0.4),
            grade: Some(0.2),
            accent_profile_ref: None,
            accent_intensity: None,
            breath_bracket: BreathBracketReq::Required,
            determinism_class: DeterminismClass::SeedReproducible,
            seed: Some(7),
        }
    }

    #[test]
    fn validation_refuses_unrepresentable_targets() {
        assert!(params().validate().is_empty());
        let mut p = params();
        p.f0_median_hz = Some(900.0);
        assert!(!p.validate().is_empty(), "out-of-range F0 refuses");
        let mut p = params();
        p.grade = Some(1.5);
        assert!(!p.validate().is_empty());
        let mut p = params();
        p.accent_profile_ref = Some("accent.x".into());
        assert!(
            !p.validate().is_empty(),
            "accent profile without intensity refuses"
        );
    }

    #[test]
    fn schema_descriptor_matches_the_freeze() {
        let descriptor: Value =
            serde_json::from_str(CONDITIONING_SCHEMA_JSON).expect("descriptor parses");
        assert_eq!(descriptor["version"], CONDITIONING_SCHEMA_VERSION);
        let props = descriptor["properties"].as_object().unwrap();
        for field in [
            "text",
            "identity_ref",
            "instruction",
            "f0_median_hz",
            "f0_range_semitones",
            "tempo_syllables_per_sec",
            "pause_structure",
            "breathiness",
            "grade",
            "accent_profile_ref",
            "accent_intensity",
            "breath_bracket",
            "determinism_class",
            "seed",
        ] {
            assert!(props.contains_key(field), "descriptor field {field}");
        }
    }

    #[test]
    fn registry_invariants() {
        let mut reg = VoiceRegistry::default();
        reg.identities.push(VoiceIdentity {
            identity_ref: "voice.a".into(),
            construction: Construction::AnchoredSynthetic { notes: "n".into() },
            locked_reference: None,
            accent_profile_ref: None,
            lock_state: LockState::Anchored,
        });
        assert!(reg.validate().is_err(), "anchored without reference fails");
        reg.identities[0].locked_reference = Some("assets/ref.wav".into());
        assert!(reg.validate().is_ok());
        assert!(reg.resolve_anchored("voice.a").is_ok());
        assert!(reg.resolve_anchored("voice.missing").is_err());
    }
}
