//! T1 gate: the served catalogue reproduces the catalogue of
//! record exactly. This test reads the source catalogue file
//! (TORORI-CATALOGUE.md) and counts independently of the shipped
//! RON data — the two must agree entry for entry.

use torori::catalogue::{Catalogue, Kind, Status};
use torori::state::Paths;

fn source_counts() -> (usize, [usize; 3], [usize; 4]) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../adult-audio-research-20261026/TORORI-CATALOGUE.md"
    );
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("catalogue of record unreadable at {path}: {e}"));
    let mut total = 0usize;
    let mut kinds = [0usize; 3]; // KINK, ACT, REACTION
    let mut statuses = [0usize; 4]; // HAVE-LIBRARY, MEASURED-ONLY, PLANNED, GAP
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("### ") {
            total += 1;
            if rest.contains("[KINK]") {
                kinds[0] += 1;
            } else if rest.contains("[ACT]") {
                kinds[1] += 1;
            } else if rest.contains("[REACTION]") {
                kinds[2] += 1;
            } else {
                panic!("heading without kind: {line}");
            }
        }
    }
    // Status counting needs entry bodies (the token may wrap
    // onto the next line): split on headings and take the first
    // status token occurring after the **Status:** marker.
    const TOKENS: [&str; 4] = ["HAVE-LIBRARY", "MEASURED-ONLY", "PLANNED", "GAP"];
    let bodies: Vec<&str> = text.split("\n### ").collect();
    let mut counted = 0usize;
    for body in bodies.iter().skip(1) {
        let Some(pos) = body.find("**Status:**") else {
            panic!("entry without status section");
        };
        let tail = &body[pos..];
        let first = TOKENS
            .iter()
            .enumerate()
            .filter_map(|(i, t)| tail.find(t).map(|p| (p, i)))
            .min_by_key(|(p, _)| *p);
        let Some((_, i)) = first else {
            panic!("status section without a token");
        };
        statuses[i] += 1;
        counted += 1;
    }
    assert_eq!(counted, total, "every entry's status counted");
    (total, kinds, statuses)
}

#[test]
fn catalogue_reproduces_the_record_exactly() {
    let (total, kinds, statuses) = source_counts();
    assert_eq!(total, 114, "the catalogue of record has 114 entries");
    let paths = Paths::resolve(
        Some(std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/data"
        ))),
        Some(std::env::temp_dir().join("torori-t1-gate")),
    );
    let catalogue = Catalogue::load(&paths).expect("catalogue loads and validates");
    assert_eq!(catalogue.entries.len(), total);
    let count_kind = |k: Kind| catalogue.entries.iter().filter(|e| e.kind == k).count();
    assert_eq!(count_kind(Kind::Kink), kinds[0], "KINK count");
    assert_eq!(count_kind(Kind::Act), kinds[1], "ACT count");
    assert_eq!(count_kind(Kind::Reaction), kinds[2], "REACTION count");
    let count_status = |s: Status| catalogue.entries.iter().filter(|e| e.status == s).count();
    assert_eq!(count_status(Status::HaveLibrary), statuses[0]);
    assert_eq!(count_status(Status::MeasuredOnly), statuses[1]);
    assert_eq!(count_status(Status::Planned), statuses[2]);
    assert_eq!(count_status(Status::Gap), statuses[3]);
    // The four HAVE-LIBRARY rows by name (coverage summary).
    let have: Vec<&str> = catalogue
        .entries
        .iter()
        .filter(|e| e.status == Status::HaveLibrary)
        .map(|e| e.title.as_str())
        .collect();
    assert_eq!(have.len(), 4);
    assert!(have.iter().any(|t| t.contains("Handcuffs")), "{have:?}");
    assert!(have.iter().any(|t| t.contains("Moaning")), "{have:?}");
    assert!(have.iter().any(|t| t.contains("Vore")), "{have:?}");
    assert!(have.iter().any(|t| t.contains("Wet friction")), "{have:?}");
}
