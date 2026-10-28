#!/usr/bin/env python3
"""Catalogue migration batch 1 (build brief T3, GAP5 §6.1).

Adds the `Pair:` / `Form:` lines to the conditioning-family
entries in data/catalogue/entries.ron. Every assignment is
listed below — this table IS the migration record. All named
pairs live in data/pairs/kit.ron (program torori.kit) and must
validate Armed-grade; the §6.1 PLANNED-requires-Armed-grade
lint is enforced at server load (src/catalogue.rs
lint_named_pairs) and by the T3 gate test.

Idempotent: entries already carrying the target values are
left untouched.
"""

import re
import sys

PATH = "data/catalogue/entries.ron"

BATCH = {
    "kink-hypnosis-conditioning": ("pair.torori.kit.hypnosis-frame — install", "Forward, Single"),
    "reaction-trigger-snap-conditioning-cue": ("pair.torori.kit.drop-snap — install", "Forward, Single"),
    "reaction-countdown-conditioning": ("pair.torori.kit.countdown — install", "Forward, Single"),
    "reaction-induction-voice-spoken": ("pair.torori.kit.induction-voice — install", "Forward, Single"),
    "act-yuri-two-women": ("pair.torori.kit.yuri-dyad — dyadic-inverse", "Inverse, Dyadic"),
    "kink-pleasure-from-pleasing": ("pair.torori.kit.pleasing-echo — install", "Forward, Single"),
    "kink-praise-kink": ("pair.torori.kit.praise-response — install", "Forward, Single"),
    "kink-voyeurism-watching-side": ("pair.torori.kit.watching-side — install", "Forward, Single"),
}


def main() -> int:
    text = open(PATH, encoding="utf-8").read()
    changed = 0
    for slug, (pair_line, form_line) in BATCH.items():
        # One entry block: from its slug line to the closing "),"
        pattern = re.compile(
            r'(slug: "' + re.escape(slug) + r'",.*?pair: )(None|Some\("[^"]*"\))(,\s*\n\s*form: )(None|Some\("[^"]*"\))',
            re.S,
        )
        m = pattern.search(text)
        if not m:
            print(f"ABORT: entry {slug} not found or shape drifted")
            return 1
        replacement = (
            m.group(1)
            + f'Some("{pair_line}")'
            + m.group(3)
            + f'Some("{form_line}")'
        )
        if m.group(0) != replacement:
            text = text[: m.start()] + replacement + text[m.end():]
            changed += 1
    open(PATH, "w", encoding="utf-8").write(text)
    print(f"batch 1 applied: {changed} entries updated, {len(BATCH)} in batch")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
