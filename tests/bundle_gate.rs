//! T3 gate: GAP5 worked records validate Armed-grade from the
//! shipped data; a neemu-shaped fixture bundle round-trips
//! validate → ingest → list byte-stably (this is the D-116
//! utilization evidence shape); each ingest-validation stage
//! refuses with its typed code.

use serde_json::json;
use torori::TororiState;
use torori::audio::{WavData, encode_wav};
use torori::bundles::{CueBundle, ProvenanceLink, Tier, TororiCue};
use torori::conditioning::{BreathBracketReq, ConditioningParams};
use torori::manifest::{AssetManifest, DeterminismClass, FinishRecord, HonestyLabel};
use torori::pairs::{ValidateCtx, validate_doc};
use torori::state::Paths;

fn repo_paths(root: &std::path::Path) -> Paths {
    Paths::resolve(
        Some(std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/data"
        ))),
        Some(root.to_path_buf()),
    )
}

#[test]
fn worked_records_validate_armed_grade() {
    let root = std::env::temp_dir().join(format!("torori-t3-docs-{}", std::process::id()));
    let paths = repo_paths(&root);
    // State load itself runs the §6.1 lint; a failure here fails
    // the gate before any assertion below.
    let state = TororiState::load(paths).expect("state loads (lint passes)");
    assert_eq!(
        state.bundles.program_docs.len(),
        3,
        "three program docs ship"
    );
    let mut pair_ids: Vec<String> = Vec::new();
    for doc in &state.bundles.program_docs {
        let ctx = ValidateCtx {
            asset_index: torori::catalogue::doc_internal_assets(doc),
            disclosures: doc
                .pairs
                .iter()
                .map(|p| p.install.effect.disclosure_ref.clone())
                .collect(),
            ship_templates: vec![
                ("T-RESET".into(), "reset".into()),
                ("T-INSTALL".into(), "install".into()),
            ],
            pair_templates: Vec::new(),
        };
        let violations = validate_doc(doc, &ctx);
        assert!(
            violations.is_empty(),
            "doc {} violations: {violations:?}",
            doc.program_id
        );
        pair_ids.extend(doc.pairs.iter().map(|p| p.pair_id.clone()));
    }
    for canonical in [
        "pair.torori.kit.drop-snap",
        "pair.moondeep.depth.t3-motes",
        "pair.moondeep.tandem.service.drop-snap",
    ] {
        assert!(
            pair_ids.iter().any(|p| p == canonical),
            "missing {canonical}"
        );
    }
    // Batch 1: all eight conditioning-family entries carry
    // Pair/Form lines now.
    let with_pair = state
        .catalogue
        .entries
        .iter()
        .filter(|e| e.pair.is_some() && e.form.is_some())
        .count();
    assert_eq!(with_pair, 8, "batch 1 covers the conditioning family");
    let _ = std::fs::remove_dir_all(&root);
}

// ── The neemu-shaped fixture bundle ───────────────────────────

struct Fixture {
    base: std::path::PathBuf,
    paths: Paths,
    bundle_dir: std::path::PathBuf,
}

fn fixture(tag: &str) -> Fixture {
    let base = std::env::temp_dir().join(format!("torori-t3-bundle-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    // A private data dir: one anchored fixture identity.
    let data = base.join("data");
    std::fs::create_dir_all(data.join("voices")).unwrap();
    std::fs::create_dir_all(data.join("pairs")).unwrap();
    std::fs::create_dir_all(data.join("catalogue")).unwrap();
    std::fs::write(data.join("voices/identities.ron"), r#"(identities: [
    (identity_ref: "voice.fixture.motte", construction: AnchoredSynthetic(notes: "T3 fixture identity"),
     locked_reference: Some("assets/fixture-ref.wav"), accent_profile_ref: None, lock_state: Anchored),
    (identity_ref: "voice.fixture.unanchored", construction: AnchoredSynthetic(notes: "designed only"),
     locked_reference: None, accent_profile_ref: None, lock_state: Designed),
])"#).unwrap();
    let paths = Paths::resolve(Some(data), Some(base.join("state")));
    paths.ensure_state_dirs().unwrap();
    // One bundle asset: a real WAV with a complete §5.2 manifest.
    let bundle_dir = base.join("bundle-src");
    std::fs::create_dir_all(bundle_dir.join("audio")).unwrap();
    let wav = WavData {
        sample_rate: 48_000,
        channels: 1,
        samples: (0..48_000)
            .map(|i| 0.2 * (2.0 * std::f64::consts::PI * 196.0 * i as f64 / 48_000.0).sin() as f32)
            .collect(),
    };
    let bytes = encode_wav(&wav);
    std::fs::write(bundle_dir.join("audio/stem.wav"), &bytes).unwrap();
    let sha = torori::util::sha256_hex(&bytes);
    let mut manifest = AssetManifest::new("fixture.stem", "bundle-fixture");
    manifest.character_id = "class".into();
    manifest.prompt = "fixture stem for bundle round-trip".into();
    manifest.parameters = json!({"fixture": true});
    manifest.rights = "estate fixture; no third-party material".into();
    manifest.determinism_class = DeterminismClass::BitExact;
    manifest.honesty_label = HonestyLabel::Authored;
    manifest.qc = Some(torori::qc::compute(&wav, None, false));
    manifest.finish = Some(FinishRecord {
        lufs: -16.0,
        true_peak_dbtp: -1.0,
        sample_rate: 48_000,
        channels: 1,
    });
    manifest.stamp_output(&bytes);
    let conditioning = ConditioningParams {
        text: "sink down now".into(),
        identity_ref: "voice.fixture.motte".into(),
        instruction: Some("slow, low, unhurried".into()),
        f0_median_hz: Some(196.0),
        f0_range_semitones: Some(3.0),
        tempo_syllables_per_sec: Some(2.8),
        pause_structure: None,
        breathiness: Some(0.3),
        grade: Some(0.2),
        accent_profile_ref: None,
        accent_intensity: None,
        breath_bracket: BreathBracketReq::Required,
        determinism_class: DeterminismClass::SeedReproducible,
        seed: Some(42),
    };
    let mut bundle = CueBundle {
        schema_version: 1,
        bundle_id: "bundle.fixture.neemu-shaped".into(),
        producer: "neemu-fixture@0.0.0+test".into(),
        cues: vec![TororiCue {
            cue_id: "cue.1".into(),
            text: "sink down now".into(),
            conditioning,
            intent: "induction".into(),
            target_voice: "voice.fixture.motte".into(),
            sfx_pairs: vec!["reaction-trigger-snap-conditioning-cue".into()],
            tier: Tier::Measured,
        }],
        pairs: vec![],
        shared_releases: vec![],
        assets: vec![torori::bundles::BundleAsset {
            asset_ref: "audio/stem.wav".into(),
            sha256: sha,
            manifest,
        }],
        provenance: vec![ProvenanceLink {
            step: "authored".into(),
            actor: "neemu-fixture".into(),
            artifact_sha256: "0".repeat(64),
        }],
    };
    let content_hash = bundle.content_hash().unwrap();
    bundle.provenance[0].artifact_sha256 = content_hash;
    let text =
        ron::ser::to_string_pretty(&bundle, ron::ser::PrettyConfig::new().struct_names(true))
            .unwrap();
    std::fs::write(bundle_dir.join("bundle.ron"), text).unwrap();
    Fixture {
        base,
        paths,
        bundle_dir,
    }
}

#[test]
fn neemu_shaped_bundle_round_trips_byte_stably() {
    let fx = fixture("rt");
    let mut state = TororiState::load(fx.paths.clone()).expect("load");
    let args = json!({
        "bundle_dir": fx.bundle_dir.to_str().unwrap(),
        "disclosures": ["disclosure.torori.kit"],
    });
    // A cue bundle with no pairs ignores disclosures; validation
    // must pass regardless.
    let validated = state
        .bundles
        .validate_tool(&state.paths, &state.voices, &args)
        .expect("validate");
    assert_eq!(validated["valid"], true);
    let hash_before = torori::util::sha256_file(&fx.bundle_dir.join("bundle.ron")).unwrap();
    let ingested = state
        .bundles
        .ingest(&state.paths, &state.voices, &args)
        .expect("ingest");
    assert_eq!(ingested["idempotent"], false);
    assert_eq!(ingested["cues"], 1);
    let stored_dir = ingested["stored_dir"].as_str().unwrap().to_string();
    // Byte-stable: the stored bundle.ron is the source's bytes.
    let hash_after =
        torori::util::sha256_file(&std::path::PathBuf::from(&stored_dir).join("bundle.ron"))
            .unwrap();
    assert_eq!(hash_before, hash_after, "stored bundle is byte-identical");
    let again = state
        .bundles
        .ingest(&state.paths, &state.voices, &args)
        .expect("re-ingest");
    assert_eq!(again["idempotent"], true, "content-hash idempotency");
    let list = state.bundles.list_bundles(&json!({})).unwrap();
    assert_eq!(list["total"], 1);
    let _ = std::fs::remove_dir_all(&fx.base);
}

#[test]
fn bundle_adversarial_refusals_are_typed_and_ordered() {
    let fx = fixture("adv");
    let mut state = TororiState::load(fx.paths.clone()).expect("load");
    let bundle_path = fx.bundle_dir.join("bundle.ron");
    let original = std::fs::read_to_string(&bundle_path).unwrap();
    let attempt = |state: &TororiState| {
        state.bundles.validate_tool(
            &state.paths,
            &state.voices,
            &json!({"bundle_dir": fx.bundle_dir.to_str().unwrap()}),
        )
    };
    // Wrong schema version → stage 1 code.
    let tampered = original.replace("schema_version: 1", "schema_version: 99");
    assert_ne!(tampered, original);
    std::fs::write(&bundle_path, &tampered).unwrap();
    assert_eq!(attempt(&state).unwrap_err().code, "bundle_schema_version");
    // Asset bytes drift → stage 2 hash code.
    std::fs::write(&bundle_path, &original).unwrap();
    std::fs::write(fx.bundle_dir.join("audio/stem.wav"), b"drifted").unwrap();
    assert_eq!(attempt(&state).unwrap_err().code, "bundle_hash");
    // Provenance final link broken → stage 3 code. (Rewrite the
    // bundle with a wrong final link via the parsed struct.)
    let wav = WavData {
        sample_rate: 48_000,
        channels: 1,
        samples: vec![0.0; 16],
    };
    let _ = wav;
    let mut bundle = torori::bundles::read_bundle(&fx.bundle_dir).unwrap();
    // Restore the asset bytes first.
    let bytes = std::fs::read(fx.base.join("bundle-src/audio/stem.wav")).unwrap_or_default();
    let _ = bytes;
    bundle.provenance[0].artifact_sha256 = "f".repeat(64);
    // The asset on disk is drifted now, which would trip stage 2
    // first — rebuild the fixture asset to isolate stage 3.
    let wav = WavData {
        sample_rate: 48_000,
        channels: 1,
        samples: (0..48_000)
            .map(|i| 0.2 * (2.0 * std::f64::consts::PI * 196.0 * i as f64 / 48_000.0).sin() as f32)
            .collect(),
    };
    std::fs::write(fx.bundle_dir.join("audio/stem.wav"), encode_wav(&wav)).unwrap();
    let text =
        ron::ser::to_string_pretty(&bundle, ron::ser::PrettyConfig::new().struct_names(true))
            .unwrap();
    std::fs::write(&bundle_path, text).unwrap();
    assert_eq!(attempt(&state).unwrap_err().code, "bundle_provenance");
    // Unanchored target voice → stage 5 code.
    bundle.provenance[0].artifact_sha256 = bundle.content_hash().unwrap();
    bundle.cues[0].target_voice = "voice.fixture.unanchored".into();
    bundle.cues[0].conditioning.identity_ref = "voice.fixture.unanchored".into();
    bundle.provenance[0].artifact_sha256 = bundle.content_hash().unwrap();
    let text =
        ron::ser::to_string_pretty(&bundle, ron::ser::PrettyConfig::new().struct_names(true))
            .unwrap();
    std::fs::write(&bundle_path, text).unwrap();
    assert_eq!(attempt(&state).unwrap_err().code, "voice_not_anchored");
    let _ = std::fs::remove_dir_all(&fx.base);
    let _ = &mut state;
}
