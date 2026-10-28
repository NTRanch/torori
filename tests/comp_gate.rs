//! T5 gate: Route C assembly end-to-end — provenance law as
//! code (laundering refusal), a shipped comp with manifest, and
//! the §4.3 gate riding on every assembly.

use serde_json::json;
use torori::TororiState;
use torori::audio::{WavData, encode_wav};
use torori::state::Paths;

fn tone_wav(freq: f64, seconds: f64, amp: f32) -> WavData {
    WavData {
        sample_rate: 48_000,
        channels: 1,
        samples: (0..(48_000.0 * seconds) as usize)
            .map(|i| amp * (2.0 * std::f64::consts::PI * freq * i as f64 / 48_000.0).sin() as f32)
            .collect(),
    }
}

fn fixture() -> (std::path::PathBuf, Paths, std::path::PathBuf) {
    let base = std::env::temp_dir().join(format!("torori-t5-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let data = base.join("data");
    std::fs::create_dir_all(data.join("voices")).unwrap();
    std::fs::create_dir_all(data.join("pairs")).unwrap();
    std::fs::create_dir_all(data.join("catalogue")).unwrap();
    let paths = Paths::resolve(Some(data), Some(base.join("state")));
    paths.ensure_state_dirs().unwrap();
    // A "library" pack clip on disk + its CLEARED row, ingested
    // through the real capture path.
    let pack_dir = base.join("pack");
    let clip_dir = pack_dir.join("Female").join("Alice");
    std::fs::create_dir_all(&clip_dir).unwrap();
    std::fs::write(
        clip_dir.join("stem.wav"),
        encode_wav(&tone_wav(196.0, 1.0, 0.25)),
    )
    .unwrap();
    std::fs::write(
        clip_dir.join("stem.wav.meta.json"),
        r#"{"license": "CC-BY 4.0", "author_credit": "Alice A", "license_source_url": "https://example.invalid/cc-by", "source_url": "https://example.invalid/stem"}"#,
    )
    .unwrap();
    (base, paths, pack_dir)
}

#[test]
fn comp_lane_end_to_end_and_laundering_refusal() {
    let (base, paths, pack_dir) = fixture();
    let mut state = TororiState::load(paths.clone()).expect("load");
    state
        .library
        .ingest(
            &paths,
            &json!({"pack_dir": pack_dir.to_str().unwrap(), "pack": "pack"}),
        )
        .expect("ingest pack");
    // A generated asset to (try to) launder.
    let gen_dir = paths.assets_dir().join("voice.fake-t0");
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(
        gen_dir.join("take.wav"),
        encode_wav(&tone_wav(220.0, 1.0, 0.2)),
    )
    .unwrap();

    // Adversarial: generated layer into a pack-derived base
    // refuses — the conjunction ban's assembly direction.
    let launder = torori::comp::assemble(
        &paths,
        &state.library,
        &json!({
            "asset_id": "comp.launder",
            "base": {"kind": "library", "pack": "pack", "file": "Female/Alice/stem.wav"},
            "layers": [{"kind": "generated", "asset_id": "voice.fake-t0", "gain_db": -12.0}],
            "cleanup": false
        }),
    );
    assert_eq!(launder.unwrap_err().code, "provenance_laundering");

    // The lawful direction: voice file base + pack clip as
    // comping material + an SFX layer under it.
    let voice_file = base.join("voice.wav");
    std::fs::write(&voice_file, encode_wav(&tone_wav(210.0, 1.0, 0.3))).unwrap();
    let out = torori::comp::assemble(
        &paths,
        &state.library,
        &json!({
            "asset_id": "comp.demo",
            "base": {"kind": "file", "path": voice_file.to_str().unwrap()},
            "layers": [
                {"kind": "library", "pack": "pack", "file": "Female/Alice/stem.wav", "gain_db": -14.0},
                {"kind": "sfx", "instrument": "room_tone", "seed": 4, "duration_sec": 1.0, "gain_db": -20.0}
            ],
            "cleanup": true
        }),
    )
    .expect("assemble");
    assert!(
        out["shipped_form"].as_str().unwrap().contains("untreated")
            || out["shipped_form"].as_str().unwrap().contains("cleaned")
    );
    assert!(
        out["gate"]["verdict"].is_string(),
        "gate numbers ride every comp: {out}"
    );
    let comp_file = std::path::PathBuf::from(out["file"].as_str().unwrap());
    assert!(comp_file.exists());
    let manifest = std::fs::read_to_string(comp_file.parent().unwrap().join("manifest.ron"))
        .expect("manifest written");
    assert!(manifest.contains("route-c-comp"), "manifest names the lane");
    let _ = std::fs::remove_dir_all(&base);
}

/// T5 reproduction gate (brief §7): the §4.3 gate must
/// reproduce the spike's Lane 1 verdict on the Betsy fixture
/// BEFORE it judges new work. The spike evaluated 8 cleanup
/// rungs; none passed all six numbers (verdict NO-SHIP; the
/// closest, SS-1, failed only the floor rung at 4.04 dB).
/// Env-gated: runs where the fixture is mounted
/// (TORORI_BETSY_DIR = the spike's out dir on primo); skips
/// silently elsewhere.
#[test]
fn betsy_fixture_reproduces_the_spike_verdict() {
    let Ok(dir) = std::env::var("TORORI_BETSY_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let untreated = torori::audio::read_wav(&dir.join("lane1_untreated_comp.wav"))
        .expect("untreated comp readable");
    for treated_name in [
        "lane1_ss_treated.wav",
        "lane1_df3_treated.wav",
        "lane1_df3al6_treated.wav",
    ] {
        let treated = torori::audio::read_wav(&dir.join(treated_name))
            .unwrap_or_else(|e| panic!("{treated_name} readable: {e:?}"));
        let n = untreated.samples.len().min(treated.samples.len());
        let numbers = torori::comp::measure_gate(
            &untreated.samples[..n],
            &treated.samples[..n],
            untreated.sample_rate,
        );
        println!(
            "BETSY {treated_name}: verdict={} floor_reduction={:.2} dB treated_floor={:.2} f0_corr={:.3} fail={:?}",
            numbers.verdict,
            numbers.floor_reduction_db,
            numbers.treated_floor_dbfs,
            numbers.f0_correlation,
            numbers.failing_rungs
        );
        assert_eq!(
            numbers.verdict, "NO-SHIP",
            "the gate must reproduce the spike's verdict on {treated_name}: {numbers:?}"
        );
    }
}

#[test]
fn sfx_render_refuses_non_bit_exact() {
    let root = std::env::temp_dir().join(format!("torori-t5b-{}", std::process::id()));
    let paths = Paths::resolve(None, Some(root.clone()));
    paths.ensure_state_dirs().unwrap();
    let out = torori::sfx::render(
        &paths,
        &json!({"instrument": "impact", "determinism_class": "SEED-REPRODUCIBLE"}),
    );
    assert_eq!(out.unwrap_err().code, "determinism_class");
    // And the lawful render is byte-stable across calls.
    let a = torori::sfx::render(
        &paths,
        &json!({"instrument": "impact", "params": {"preset": "crop", "force": 0.7}, "seed": 3, "duration_sec": 0.4}),
    )
    .unwrap();
    let b = torori::sfx::render(
        &paths,
        &json!({"instrument": "impact", "params": {"preset": "crop", "force": 0.7}, "seed": 3, "duration_sec": 0.4}),
    )
    .unwrap();
    assert_eq!(a["sha256"], b["sha256"], "double render is bit-exact");
    let _ = std::fs::remove_dir_all(&root);
}
