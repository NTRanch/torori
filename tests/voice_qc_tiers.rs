//! S2 wiring gate (D-483): `torori_voice_qc` carries the
//! Tier 2 drift alarm on every QC record (alarm-only, cosine
//! never fabricated), and `source_text` triggers the Tier 1
//! ASR round-trip — a fake ASR driver stands in for the Qwen
//! process so the wiring is gated without a GPU. Asking for
//! Tier 1 with no ASR driver configured is the typed
//! `asr_unconfigured` refusal, never a silently skipped tier.
//!
//! Own test file = own process: the lane's env configuration
//! (TORORI_QWEN_DRIVER / TORORI_ASR_DRIVER) is process-global.

use serde_json::json;
use torori::TororiState;
use torori::audio::{WavData, encode_wav};
use torori::state::Paths;

#[test]
fn voice_qc_tier1_tier2_wiring() {
    let base = std::env::temp_dir().join(format!("torori-s2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let data = base.join("data");
    std::fs::create_dir_all(data.join("voices")).unwrap();
    std::fs::create_dir_all(data.join("pairs")).unwrap();
    std::fs::create_dir_all(data.join("catalogue")).unwrap();
    std::fs::write(
        data.join("voices/identities.ron"),
        r#"(identities: [
    (identity_ref: "voice.fixture.motte", construction: VoiceDesign(record_ref: "design.fixture.1", seed_manifest_ref: "seedman.fixture.1"),
     locked_reference: None, accent_profile_ref: None, lock_state: Designed),
])"#,
    )
    .unwrap();
    let wav = WavData {
        sample_rate: 24_000,
        channels: 1,
        samples: (0..24_000)
            .map(|i| 0.25 * (2.0 * std::f64::consts::PI * 210.0 * i as f64 / 24_000.0).sin() as f32)
            .collect(),
    };
    let fake_wav = base.join("fake-take.wav");
    std::fs::write(&fake_wav, encode_wav(&wav)).unwrap();
    let weights = "b".repeat(64);
    let driver = base.join("fake-driver.sh");
    std::fs::write(
        &driver,
        format!(
            "#!/bin/sh\nout=\"\"\nwhile [ $# -gt 0 ]; do if [ \"$1\" = \"--out\" ]; then out=\"$2\"; fi; shift; done\nmkdir -p \"$out\"\ncp \"{}\" \"$out/take0.wav\"\nprintf '{{\"model\": \"Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign\", \"weights_sha256\": \"{}\", \"sample_rate\": 24000, \"takes\": [{{\"file\": \"take0.wav\", \"seed\": 34001}}]}}' > \"$out/result.json\"\n",
            fake_wav.display(),
            weights
        ),
    )
    .unwrap();
    // The fake ASR driver: transcribes the line perfectly.
    let asr_driver = base.join("fake-asr.sh");
    std::fs::write(
        &asr_driver,
        "#!/bin/sh\nprintf '{\"model\": \"Qwen/Qwen3-ASR-0.6B\", \"transcript\": \"close your eyes and breathe\"}\\n'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for f in [&driver, &asr_driver] {
            std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    // SAFETY: this test process owns its env; vars are set
    // before any lane call in this process.
    unsafe {
        std::env::set_var(
            "TORORI_QWEN_DRIVER",
            format!("/bin/sh {}", driver.display()),
        );
        std::env::remove_var("TORORI_ASR_DRIVER");
    }
    let paths = Paths::resolve(Some(data), Some(base.join("state")));
    let state = TororiState::load(paths).expect("load");
    let args = json!({
        "slot_class": "induction",
        "takes": 1,
        "conditioning": {
            "text": "close your eyes and breathe",
            "identity_ref": "voice.fixture.motte",
            "instruction": "a calm adult woman's voice, slow and soft",
            "f0_median_hz": 210.0, "f0_range_semitones": 3.0,
            "tempo_syllables_per_sec": 2.8, "pause_structure": null,
            "breathiness": 0.3, "grade": 0.2,
            "accent_profile_ref": null, "accent_intensity": null,
            "breath_bracket": "Required",
            "determinism_class": "SEED-REPRODUCIBLE", "seed": 34001
        }
    });
    let out = torori::voice::generate(&state.paths, &state.voices, &args).expect("generate");
    let asset_id = out["assets"][0]["asset_id"].as_str().unwrap().to_string();

    // Validation: without source_text, Tier 1 is absent and
    // the Tier 2 alarm rides on the QC record — inert for a
    // Designed identity, cosine null (never fabricated).
    let qc = torori::voice::voice_qc(&state.paths, &state.voices, &json!({"asset_id": asset_id}))
        .expect("voice_qc");
    assert!(qc["tier1"].is_null(), "{qc}");
    let drift = &qc["qc"]["speaker_drift"];
    assert_eq!(drift["status"], "inert-no-locked-anchor", "{qc}");
    assert!(
        drift["cosine"].is_null(),
        "cosine is never fabricated: {qc}"
    );
    assert!(drift["anchor_ref"].is_null(), "{qc}");

    // Adversarial: Tier 1 requested with no ASR driver
    // configured refuses typed — the measurement is never
    // silently skipped.
    let err = torori::voice::voice_qc(
        &state.paths,
        &state.voices,
        &json!({"asset_id": asset_id, "source_text": "close your eyes and breathe"}),
    )
    .unwrap_err();
    assert_eq!(err.code, "asr_unconfigured");

    // Validation: with the fake ASR driver configured, the
    // round-trip runs and scores the exact transcript at 0.
    unsafe {
        std::env::set_var(
            "TORORI_ASR_DRIVER",
            format!("/bin/sh {}", asr_driver.display()),
        );
    }
    let qc = torori::voice::voice_qc(
        &state.paths,
        &state.voices,
        &json!({"asset_id": asset_id, "source_text": "close your eyes and breathe"}),
    )
    .expect("voice_qc with tier1");
    assert_eq!(qc["tier1"]["wer"], 0.0, "{qc}");
    assert_eq!(qc["tier1"]["verdict"], "Ship", "{qc}");
    assert_eq!(qc["tier1"]["bars_provisional"], true, "{qc}");
    assert_eq!(qc["tier1"]["model"], "Qwen/Qwen3-ASR-0.6B", "{qc}");

    // Adversarial: a non-string source_text is invalid input.
    let err = torori::voice::voice_qc(
        &state.paths,
        &state.voices,
        &json!({"asset_id": asset_id, "source_text": 42}),
    )
    .unwrap_err();
    assert_eq!(err.code, "invalid_input");

    unsafe {
        std::env::remove_var("TORORI_QWEN_DRIVER");
        std::env::remove_var("TORORI_ASR_DRIVER");
    }
    let _ = std::fs::remove_dir_all(&base);
}
