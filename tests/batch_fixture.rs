//! S3 end-to-end fixture (D-483): the 14 S1-delivered assets
//! (two takes of the anchor line + 12 script lines) treated as
//! one scene — the batch layer must select one take per line,
//! refuse nothing without bookkeeping, and emit cues whose
//! text is the script verbatim.
//!
//! Inputs (both optional; the test skips with a note when the
//! fixture is absent, e.g. a fresh clone):
//! - Fixture dir: env TORORI_S1_FIXTURE_DIR, else the sibling
//!   `../s1-delivery` next to this crate.
//! - Tier 1 map: env TORORI_TIER1_FIXTURE — a JSON object
//!   {asset_id: Tier1Record} produced by running the real
//!   Tier 1 lane over the fixture (primo, venv-qwen-asr).
//! - When env TORORI_BATCH_OUT is set, the assembly's three
//!   RON files are written there as gate evidence.

use std::collections::BTreeMap;
use std::path::PathBuf;

use torori::asr::Tier1Record;
use torori::batch::{SceneLine, assemble, load_delivery_dir, write_assembly};

const IDENTITY: &str = "voice.trial.prima-1";

/// The 13 distinct S1 lines, verbatim from the delivery
/// (s1-delivery/METRICS.md; line_id = delivery file stem).
/// Lines 11–12 carry their SSML-lite tags verbatim, exactly
/// as fed at S1 (tree bda4028 has no tag parse/strip layer).
const SCRIPT: [(&str, &str); 13] = [
    (
        "00-anchor",
        "Welcome back. Settle in, get comfortable, and let me walk you through everything that happens next — slowly, clearly, and in order.",
    ),
    (
        "01-neutral",
        "The archive keeps three copies of every record: one on paper, one on disk, and one nobody talks about.",
    ),
    (
        "02-neutral",
        "Trains leave the north platform every forty minutes, weather permitting.",
    ),
    (
        "03-warm",
        "You did well today. Better than you think — and I don't say that to just anyone.",
    ),
    (
        "04-warm",
        "Come here. Sit with me a while; the kettle's on, and there's nowhere else we need to be.",
    ),
    (
        "05-firm",
        "No. We do it properly this time, from the beginning, and we do not skip steps.",
    ),
    (
        "06-firm",
        "Listen to me. You will knock twice, say your name, and wait to be invited in. Nothing else.",
    ),
    (
        "07-bright",
        "Oh, that's wonderful news! Tell me everything — start from the very beginning!",
    ),
    (
        "08-bright",
        "Look at that — first snow, and the whole valley lit up like a postcard.",
    ),
    (
        "09-neutral",
        "If the courier comes before noon, sign for the parcel and leave it by the study door.",
    ),
    (
        "10-warm",
        "I kept your seat by the window. Some things you don't give away, even when you're cross.",
    ),
    (
        "11-ssml-breath",
        "[breath] There. Steady now — in through the nose, out through the mouth, just like we practised.",
    ),
    (
        "12-ssml-laugh",
        "[laugh] You should have seen his face when the cake came out — absolutely priceless.",
    ),
];

#[test]
fn s1_fixture_scene_assembly() {
    let fixture_dir = std::env::var("TORORI_S1_FIXTURE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("s1-delivery")
        });
    if !fixture_dir.is_dir() {
        eprintln!(
            "S1 fixture absent at {} — skipping (set TORORI_S1_FIXTURE_DIR)",
            fixture_dir.display()
        );
        return;
    }
    let takes = load_delivery_dir(&fixture_dir).expect("fixture manifests load");
    assert_eq!(takes.len(), 14, "the S1 delivery holds 14 assets");

    let tier1: BTreeMap<String, Tier1Record> = match std::env::var("TORORI_TIER1_FIXTURE") {
        Ok(path) => {
            let text = std::fs::read_to_string(&path).expect("read tier1 fixture");
            serde_json::from_str(&text).expect("tier1 fixture parses")
        }
        Err(_) => BTreeMap::new(),
    };

    let script: Vec<SceneLine> = SCRIPT
        .iter()
        .map(|(id, text)| SceneLine {
            line_id: (*id).to_string(),
            text_ref: (*id).to_string(),
            text: (*text).to_string(),
        })
        .collect();
    let assembly = assemble(
        "s1-fixture-scene",
        IDENTITY,
        &script,
        &takes,
        &tier1,
        300,
        None,
    )
    .expect("assembly");

    // Every line is either selected or refused-with-queue —
    // nothing vanishes silently.
    assert_eq!(
        assembly.selected.len() + assembly.regen_queue.len(),
        13,
        "13 lines accounted for: selected {:?}, queued {:?}",
        assembly
            .selected
            .iter()
            .map(|s| &s.line_id)
            .collect::<Vec<_>>(),
        assembly
            .regen_queue
            .iter()
            .map(|q| &q.line_id)
            .collect::<Vec<_>>(),
    );
    // One cue per selected line; cue text is the script
    // verbatim (English, D-472); cues never overlap.
    assert_eq!(assembly.cues.len(), assembly.selected.len());
    for row in &assembly.cues {
        let (_, text) = SCRIPT.iter().find(|(id, _)| *id == row.line_id).unwrap();
        assert_eq!(row.text, *text, "cue text is the source line verbatim");
        assert!(row.end_ms > row.start_ms);
    }
    for pair in assembly.cues.windows(2) {
        assert!(
            pair[1].start_ms >= pair[0].end_ms,
            "sequential cues do not overlap"
        );
    }
    // Both anchor takes compete for the same line; exactly one
    // of them is selected for it.
    let anchor = assembly
        .selected
        .iter()
        .find(|s| s.line_id == "00-anchor")
        .expect("anchor line selected (both takes passed Tier 0 at S1)");
    assert!(
        anchor.asset_id == "voice.trial.prima-1-affirmation-7374797a-t0"
            || anchor.asset_id == "voice.trial.prima-1-affirmation-5caba8af-t1",
        "anchor selected from its two takes, got {}",
        anchor.asset_id
    );

    if let Ok(out) = std::env::var("TORORI_BATCH_OUT") {
        write_assembly(&PathBuf::from(out), &assembly).expect("write assembly evidence");
    }
    eprintln!(
        "S1 fixture assembly: {} selected, {} queued, {} refusals, batch mean WER {:?}",
        assembly.selected.len(),
        assembly.regen_queue.len(),
        assembly.refusals.len(),
        assembly.batch_mean_wer
    );
}
