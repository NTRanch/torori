//! T4 gate (lane machinery): a deterministic fake driver stands
//! in for the Qwen process and the full server-side lane runs —
//! registry resolution, blocklist screen, manifest completeness
//! with a pinned weights hash, QC for 100% of takes, the asset
//! index, and the golden-delta path. The real-model run on primo
//! is reported separately in TORORI-T4-REPORT.md.

use serde_json::json;
use torori::TororiState;
use torori::audio::{WavData, encode_wav};
use torori::state::Paths;

#[test]
fn voice_lane_end_to_end_with_fake_driver() {
    let base = std::env::temp_dir().join(format!("torori-t4-{}", std::process::id()));
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
    // The fake driver's take: a 210 Hz tone WAV.
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
            "#!/bin/sh\nout=\"\"\nwhile [ $# -gt 0 ]; do if [ \"$1\" = \"--out\" ]; then out=\"$2\"; fi; shift; done\nmkdir -p \"$out\"\ncp \"{}\" \"$out/take0.wav\"\ncp \"{}\" \"$out/take1.wav\"\nprintf '{{\"model\": \"Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign\", \"weights_sha256\": \"{}\", \"sample_rate\": 24000, \"takes\": [{{\"file\": \"take0.wav\", \"seed\": 34001}}, {{\"file\": \"take1.wav\", \"seed\": 34002}}]}}' > \"$out/result.json\"\n",
            fake_wav.display(),
            fake_wav.display(),
            weights
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&driver, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // SAFETY: this integration-test process owns its env; the
    // var is set before any lane call in this process.
    unsafe {
        std::env::set_var(
            "TORORI_QWEN_DRIVER",
            format!("/bin/sh {}", driver.display()),
        );
    }
    let paths = Paths::resolve(Some(data), Some(base.join("state")));
    let state = TororiState::load(paths).expect("load");
    let args = json!({
        "slot_class": "induction",
        "takes": 2,
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
    assert_eq!(out["assets"].as_array().unwrap().len(), 2);
    assert!(
        out["grading"].as_str().unwrap().contains("UNGRADED"),
        "assets stay UNGRADED: {out}"
    );
    assert_eq!(out["weights_sha256"], weights);
    let first = &out["assets"][0];
    assert!(
        first["qc"]["f0_median_hz"].is_number(),
        "QC present: {first}"
    );
    let asset_id = first["asset_id"].as_str().unwrap().to_string();
    // Manifest on disk validates complete (§5.2 gate).
    let qc =
        torori::voice::voice_qc(&state.paths, &json!({"asset_id": asset_id})).expect("voice_qc");
    assert_eq!(qc["manifest_complete"], true, "{qc}");
    assert!(
        qc["golden_delta"].is_object(),
        "second take serves as golden: {qc}"
    );
    // Adversarial: blocklisted instruction refuses before the
    // driver ever runs.
    let mut bad = args.clone();
    bad["conditioning"]["instruction"] = json!("a schoolgirl voice");
    let refused = torori::voice::generate(&state.paths, &state.voices, &bad);
    assert_eq!(refused.unwrap_err().code, "rights_blocklist");
    // Adversarial: unrepresentable conditioning refuses (no clamp).
    let mut bad2 = args.clone();
    bad2["conditioning"]["grade"] = json!(1.7);
    let refused = torori::voice::generate(&state.paths, &state.voices, &bad2);
    assert_eq!(refused.unwrap_err().code, "conditioning_invalid");
    unsafe {
        std::env::remove_var("TORORI_QWEN_DRIVER");
    }
    let _ = std::fs::remove_dir_all(&base);
}
