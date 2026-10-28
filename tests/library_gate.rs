//! T2 gate: the library lane end-to-end on a fixture tree —
//! ingest capture discipline, decide() refusals, the fetch +
//! attribution Siamese write, and hash-drift refusal.

use serde_json::json;
use torori::TororiState;
use torori::audio::{WavData, encode_wav};
use torori::library::{LibraryStore, LicenseState};
use torori::state::Paths;

fn fixture() -> (std::path::PathBuf, Paths) {
    let base = std::env::temp_dir().join(format!("torori-t2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let pack_dir = base.join("sfx-test");
    let wav = WavData {
        sample_rate: 48_000,
        channels: 1,
        samples: vec![0.1; 4_800],
    };
    let bytes = encode_wav(&wav);
    for (performer, clip, sidecar) in [
        (
            "Alice",
            "clip-a.wav",
            Some(
                r#"{"license": "CC-BY 4.0", "license_source_url": "https://example.invalid/cc-by", "author_credit": "Alice A", "source_url": "https://example.invalid/a"}"#,
            ),
        ),
        (
            "Bob",
            "clip-b.wav",
            Some(r#"{"license": "CC-BY-NC 4.0", "author_credit": "Bob B"}"#),
        ),
        ("Carol", "clip-c.wav", None),
    ] {
        let dir = pack_dir.join("Female").join(performer);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(clip), &bytes).unwrap();
        if let Some(meta) = sidecar {
            std::fs::write(dir.join(format!("{clip}.meta.json")), meta).unwrap();
        }
    }
    let root = base.join("state");
    let paths = Paths::resolve(None, Some(root));
    (base, paths)
}

fn state_with_ingest(paths: &Paths, pack_dir: &std::path::Path) -> TororiState {
    let mut state = TororiState::load(paths.clone()).expect("load");
    let out = state
        .library
        .ingest(
            paths,
            &json!({"pack_dir": pack_dir.to_str().unwrap(), "pack": "sfx-test"}),
        )
        .expect("ingest");
    assert_eq!(out["added"], 3);
    assert_eq!(out["states"]["CLEARED"], 1);
    assert_eq!(out["states"]["NOT CLEARED"], 1);
    assert_eq!(out["states"]["UNDETERMINED"], 1);
    state
}

#[test]
fn library_lane_end_to_end() {
    let (base, paths) = fixture();
    let pack_dir = base.join("sfx-test");
    let state = state_with_ingest(&paths, &pack_dir);
    let store: &LibraryStore = &state.library;
    assert_eq!(
        store
            .find("sfx-test", "Female/Alice/clip-a.wav")
            .unwrap()
            .license_state,
        LicenseState::Cleared
    );
    assert_eq!(
        store
            .find("sfx-test", "Female/Bob/clip-b.wav")
            .unwrap()
            .license_state,
        LicenseState::NotCleared
    );
    assert_eq!(
        store
            .find("sfx-test", "Female/Carol/clip-c.wav")
            .unwrap()
            .license_state,
        LicenseState::Undetermined
    );

    // Fetch the CLEARED clip: file + manifest entry, same op.
    let out = store
        .fetch(
            &paths,
            &json!({"pack": "sfx-test", "file": "Female/Alice/clip-a.wav", "project": "demo"}),
        )
        .expect("fetch cleared");
    let staged = std::path::PathBuf::from(out["project_file"].as_str().unwrap());
    assert!(staged.exists(), "fetched file exists in staging");
    assert_eq!(
        out["attribution"]["attribution_line"],
        "Alice A — via sfx-test"
    );
    let manifest = store
        .manifest(&paths, &json!({"project": "demo"}))
        .expect("manifest");
    assert_eq!(manifest["total"], 1);
    assert_eq!(manifest["items"][0]["file"], "Female/Alice/clip-a.wav");
    assert_eq!(manifest["items"][0]["sha256"], out["sha256"]);

    // Adversarial: NOT CLEARED and UNDETERMINED refuse.
    let nc = store.fetch(
        &paths,
        &json!({"pack": "sfx-test", "file": "Female/Bob/clip-b.wav", "project": "demo"}),
    );
    assert_eq!(nc.unwrap_err().code, "license_not_cleared");
    let un = store.fetch(
        &paths,
        &json!({"pack": "sfx-test", "file": "Female/Carol/clip-c.wav", "project": "demo"}),
    );
    assert_eq!(un.unwrap_err().code, "license_undetermined");

    // Adversarial: a drifted library file refuses (hash mismatch).
    let alice = pack_dir.join("Female/Alice/clip-a.wav");
    std::fs::write(&alice, b"tampered").unwrap();
    let drift = store.fetch(
        &paths,
        &json!({"pack": "sfx-test", "file": "Female/Alice/clip-a.wav", "project": "demo2"}),
    );
    assert_eq!(drift.unwrap_err().code, "hash_mismatch");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn no_bulk_export_tool_exists() {
    let names: Vec<&str> = torori::server::tool_specs()
        .iter()
        .map(|s| s.name)
        .collect();
    for name in names {
        let lower = name.to_lowercase();
        assert!(
            !lower.contains("export") && !lower.contains("download") && !lower.contains("bulk"),
            "bulk-export class tool must never exist: {name}"
        );
    }
}
