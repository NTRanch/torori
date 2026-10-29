# Changelog — torori

All user-facing changes to the torori sound MCP, newest first.
Versions follow the estate fleet line: the T0–T6 build shipped
as 0.0.1; 1.0.0 is the close-out release (same 19-tool surface,
docs brought current, consumer-session evidence filed).

## 1.0.0 — 2026-10-28

Close-out release. No surface changes: the 19 tools, their
schemas, annotations, and per-tool timeouts are exactly the
0.0.1 surface, and the T3 freeze artefacts are unchanged
(conditioning schema v1 sha256 `e0df698d…`, cue-bundle schema
v1 sha256 `f423f883…`).

- Version bumped 0.0.1 → 1.0.0 (`Cargo.toml` + lockfile).
- Added `COMPLIANCE.md`: the compliance record written current
  against the landed 19-tool surface (modern-era rmcp posture,
  licence-state `decide()` gate, rights law, audio silence
  rule, determinism contract, deferred list).
- Added `UTILIZATION.md`: D-116 consumer-session evidence —
  one MCP-Inspector-style session (discover, tools/list,
  representative tool calls, legacy-`initialize` refusal) and
  one game-shaped production session (cue-bundle validate →
  ingest → list plus a deterministic SFX render), with full
  wire transcripts filed beside it.
- This changelog added; the 0.0.1 history below is recorded
  from the T0–T6 build record.

Deferred, carried verbatim from the build record (not 1.0.0
blockers): the two T4 trial voice takes remain UNGRADED
(Owner ear act); the VoiceIdentity registry ships empty by
design (Owner canon acts); the OpenNSFW SFX pack (B-5) is not
ingested; nonverbal graded arcs remain UNPROVEN on every lane
and their catalogue entries stay GAP.

## 0.0.1 — 2026-10-27

First gated build (T0–T6 complete; sandbox 55/55, primo
55/55; clippy `-D warnings` clean; fmt clean; Diver gate
25/25 files, 0 diagnostics). Built on rmcp 3.8.0, modern MCP
era only (2026-07-28) from birth: sessions open with
`server/discover`; legacy `initialize` contact is refused at
the stdio era guard with `-32022`.

- **T0 — skeleton + era guard.** Fleet guard template (framing
  contract, 4 MiB line bound, response-ordering tracker),
  annotations from birth, per-tool timeouts documented in
  `tool_specs()`.
- **T1 — catalogue core.** The 114-entry TORORI catalogue
  (Kink 57 / Act 34 / Reaction 23; HAVE-LIBRARY 4 /
  MEASURED-ONLY 7 / PLANNED 40 / GAP 63) served by
  `torori_catalogue_search` / `_get` / `_stats`, with the
  governed status machine in `torori_catalogue_transition`
  (GENERATED requires asset + §5.2 manifest refs; leaving
  GENERATED requires an Owner ear verdict; provenance class
  is never overwritten).
- **T2 — OpenNSFW library lane.** `torori_library_search` /
  `_fetch` / `_manifest` / `_ingest` over the per-file licence
  INDEX (7,584-row production backfill, all CLEARED). Fetch
  copies one CLEARED file out with its attribution entry in
  the same operation; UNDETERMINED / NOT CLEARED rows and
  sha256-drifted files refuse. No bulk export exists, by
  design.
- **T3 — pairs + cue bundles.** GAP5 TriggerPair validation
  (rules R1–R7, battery V1–V8) via `torori_pair_validate` /
  `_list`, and cue-bundle ingest via `torori_bundle_validate`
  / `_ingest` / `_list` (quarantine-copy, validate, promote;
  idempotent on content hash; partial ingest never happens).
  Freeze artefacts: conditioning schema v1 and cue-bundle
  schema v1 (checksums in the 1.0.0 entry above) — the
  torori↔neemu transport contract.
- **T4 — voice lane.** `torori_voice_list` / `_generate` /
  `_qc`: Qwen3-TTS generation behind the VoiceIdentity
  registry, file refs + §5.3 QC metrics only — nothing is
  played. Two real trial takes were produced on primo with
  complete §5.2 manifests; they ship UNGRADED.
- **T5 — DSP SFX + Route C comp.** `torori_sfx_render`
  (deterministic DSP instruments, BIT-EXACT only) and
  `torori_comp_assemble` (Route C mix under the §4.3 numeric
  ship gate; NO-SHIP returns the untreated comp with failing
  numbers; generated layers into a library base refuse).
- **T6 — hardening + EFLO.** Fault injection passes on all
  19 tools (malformed input returns `isError` — no hangs, no
  panics, no protocol-error leaks); `tools/list` measured at
  2,454 tokens against the 2,500 cap with a real tokenizer
  (first measure 2,604; descriptions compressed, contract
  facts kept, re-measured). Record in `EFLO.md`.

Standing law shipped with 0.0.1 and unchanged in 1.0.0: the
audio silence rule (no playback path anywhere in the code),
the rights law (no real-person voice likeness; pack clips are
comping material only), and the determinism contract (every
asset declares BIT-EXACT, SEED-REPRODUCIBLE, or OUTPUT-UNIQUE
and is never described above its class).
