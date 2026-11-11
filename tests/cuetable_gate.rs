//! TR-2 gate: a cue table's event frames agree with what is
//! measured from the audio files themselves — event-to-frame
//! error ≤ 1 frame at 60 Hz, measured from files, never by
//! playing anything (the AUDIO SILENCE RULE: files + metrics
//! only).
//!
//! The fixture is built from files both ways: a WAV whose tone
//! onset sits at exactly 0.5 s, and a cue-table RON naming that
//! onset at frame 30 with `at_sec: 0.5`. The measurement path
//! reads the WAV back, finds the onset sample, converts it to
//! the 60 Hz frame grid, and compares.

use torori::audio::{WavData, encode_wav, read_wav};
use torori::cuetable::{CueTable, frame_at};

const FPS: u32 = 60;
const ONSET_SEC: f64 = 0.5;

fn fixture_dir() -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!("torori-tr2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

fn write_fixture_wav(dir: &std::path::Path) -> std::path::PathBuf {
    let rate = 48_000u32;
    let onset = (ONSET_SEC * rate as f64) as usize;
    let mut samples = vec![0.0f32; onset];
    samples.extend((0..rate / 2).map(|i| {
        0.25 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / rate as f64).sin() as f32
    }));
    let wav = WavData {
        sample_rate: rate,
        channels: 1,
        samples,
    };
    let path = dir.join("onset.wav");
    std::fs::write(&path, encode_wav(&wav)).unwrap();
    path
}

fn write_table(dir: &std::path::Path, frame: u64, at_sec: f64) -> std::path::PathBuf {
    let path = dir.join("cues.ron");
    std::fs::write(
        &path,
        format!(
            "(schema_version: 1, table_id: \"tr2-fixture\", fps: {FPS}, events: [\
             (event_id: \"tone-onset\", frame: {frame}, kind: \"onset\", \
             slug: None, asset_ref: Some(\"onset.wav\"), at_sec: Some({at_sec}))])"
        ),
    )
    .unwrap();
    path
}

/// First sample index whose magnitude clears the threshold —
/// the measured onset, from the file alone.
fn measured_onset_frame(wav_path: &std::path::Path) -> u64 {
    let wav = read_wav(wav_path).expect("fixture wav reads");
    let mono = wav.mono();
    let index = mono
        .iter()
        .position(|s| s.abs() > 0.05)
        .expect("tone onset is present in the file");
    frame_at(index as f64 / f64::from(wav.sample_rate), FPS).expect("onset time converts")
}

#[test]
fn cue_table_event_matches_the_measured_onset_within_one_frame() {
    let dir = fixture_dir();
    let wav_path = write_fixture_wav(&dir);
    let table_path = write_table(&dir, 30, ONSET_SEC);
    let table = CueTable::load(&table_path).expect("cue table loads and validates");
    assert_eq!(table.fps, FPS);
    let event = table.get("tone-onset").expect("event present");
    let measured = measured_onset_frame(&wav_path);
    let error = event.frame.abs_diff(measured);
    assert!(
        error <= 1,
        "event frame {} vs measured frame {measured}: error {error} exceeds 1 frame at {FPS} Hz",
        event.frame
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cue_table_refuses_an_event_two_frames_from_its_authored_time() {
    let dir = fixture_dir();
    // Frame 40 against an authored 0.5 s (frame 30): 10 frames
    // out — far beyond the 1-frame budget, refused by name.
    let table_path = write_table(&dir, 40, ONSET_SEC);
    let out = CueTable::load(&table_path);
    assert_eq!(out.unwrap_err().code, "cue_table_timing");
    let _ = std::fs::remove_dir_all(&dir);
}
