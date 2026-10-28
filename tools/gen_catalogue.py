#!/usr/bin/env python3
"""Derive data/catalogue/entries.ron from the catalogue of record.

Source of record: ~/workspace/adult-audio-research-20261026/TORORI-CATALOGUE.md
(114 `### ` entries). The derivation is mechanical: heading ->
slug/title/kind/brief; body prose -> description + the
**Assets:** / **Coupling:** / **Source:** / **Status:** sections
(the status token is the first word of the status section and
must be one of the four catalogue statuses — anything else is
drift and aborts the derivation loudly).

Run: python3 tools/gen_catalogue.py [catalogue.md] [out.ron]
The output is deterministic: entries in source order, RON
strings escaped canonically.
"""

import re
import sys
import unicodedata

DEFAULT_SRC = (
    "/home/hatch/workspace/adult-audio-research-20261026/TORORI-CATALOGUE.md"
)
DEFAULT_OUT = "data/catalogue/entries.ron"

KIND = {"KINK": "Kink", "ACT": "Act", "REACTION": "Reaction"}
STATUS = {
    "HAVE-LIBRARY": "HaveLibrary",
    "MEASURED-ONLY": "MeasuredOnly",
    "PLANNED": "Planned",
    "GAP": "Gap",
}
FIELDS = ["Assets", "Coupling", "Source", "Status"]


def slugify(title: str) -> str:
    text = unicodedata.normalize("NFKD", title).encode("ascii", "ignore").decode()
    text = re.sub(r"[^a-zA-Z0-9]+", "-", text.lower()).strip("-")
    return text


def ron_str(text: str) -> str:
    out = text.replace("\\", "\\\\").replace('"', '\\"')
    out = out.replace("\n", " ").replace("\r", " ")
    out = re.sub(r"\s+", " ", out).strip()
    return f'"{out}"'


def ron_opt(text):
    if text is None or text == "":
        return "None"
    return f"Some({ron_str(text)})"


def split_sections(body: str):
    """Split entry body prose into (description, {field: text})."""
    marks = []
    for field in FIELDS:
        for m in re.finditer(r"\*\*" + field + r":\*\*", body):
            marks.append((m.start(), m.end(), field))
    marks.sort()
    if not marks:
        return body.strip(), {}
    description = body[: marks[0][0]].strip()
    sections = {}
    for i, (start, end, field) in enumerate(marks):
        stop = marks[i + 1][0] if i + 1 < len(marks) else len(body)
        sections[field] = body[end:stop].strip()
    return description, sections


def main() -> int:
    src = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_SRC
    out_path = sys.argv[2] if len(sys.argv) > 2 else DEFAULT_OUT
    text = open(src, encoding="utf-8").read()

    heading_re = re.compile(
        r"^### (.+?) \[(KINK|ACT|REACTION)\](?: — (B-\d+))?\s*$", re.M
    )
    matches = list(heading_re.finditer(text))
    if len(matches) != 114:
        print(f"ABORT: expected 114 ### entries, found {len(matches)}")
        return 1

    entries = []
    seen_slugs = set()
    for i, m in enumerate(matches):
        title, kind, brief = m.group(1).strip(), m.group(2), m.group(3)
        stop = matches[i + 1].start() if i + 1 < len(matches) else len(text)
        body = text[m.end():stop]
        # An entry's body ends at a horizontal rule / next section.
        body = body.split("\n---")[0].strip()
        description, sections = split_sections(body)
        status_text = sections.get("Status", "")
        token_match = re.match(r"(HAVE-LIBRARY|MEASURED-ONLY|PLANNED|GAP)\b", status_text)
        if not token_match:
            print(f"ABORT: entry {title!r} has no status token: {status_text[:60]!r}")
            return 1
        token = token_match.group(1)
        status_note = status_text[token_match.end():].strip(" .").strip()
        # Slug: kind prefix + title slug; collisions abort.
        slug = f"{kind.lower()}-{slugify(title)}"
        if slug in seen_slugs:
            print(f"ABORT: slug collision {slug}")
            return 1
        seen_slugs.add(slug)
        entries.append(
            dict(
                slug=slug,
                title=title,
                kind=KIND[kind],
                brief=brief,
                description=description,
                assets=sections.get("Assets"),
                coupling=sections.get("Coupling"),
                source=sections.get("Source"),
                status=STATUS[token],
                status_note=status_note or None,
            )
        )

    lines = [
        "// TORORI catalogue data — derived from TORORI-CATALOGUE.md",
        "// (catalogue of record, 2026-10-26) by tools/gen_catalogue.py.",
        "// Do not hand-edit; regenerate. Load-time validation in",
        "// src/catalogue.rs enforces kind/status shape and B-xx span.",
        "(",
        "    entries: [",
    ]
    for e in entries:
        lines.append("        (")
        lines.append(f"            slug: {ron_str(e['slug'])},")
        lines.append(f"            title: {ron_str(e['title'])},")
        lines.append(f"            kind: {e['kind']},")
        lines.append(f"            brief: {ron_opt(e['brief'])},")
        lines.append(f"            description: {ron_str(e['description'])},")
        lines.append(f"            assets: {ron_opt(e['assets'])},")
        lines.append(f"            coupling: {ron_opt(e['coupling'])},")
        lines.append(f"            source: {ron_opt(e['source'])},")
        lines.append(f"            status: {e['status']},")
        lines.append(f"            status_note: {ron_opt(e['status_note'])},")
        lines.append("            pair: None,")
        lines.append("            form: None,")
        lines.append("        ),")
    lines.append("    ],")
    lines.append(")")
    with open(out_path, "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines) + "\n")
    kinds = {}
    statuses = {}
    for e in entries:
        kinds[e["kind"]] = kinds.get(e["kind"], 0) + 1
        statuses[e["status"]] = statuses.get(e["status"], 0) + 1
    print(f"wrote {out_path}: {len(entries)} entries; kinds={kinds}; statuses={statuses}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
