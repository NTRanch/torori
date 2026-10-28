# torori — EFLO record

Opened at T0 (build brief §12). Populated from gate evidence
alone; the final roll-up lands at T6.

## Budgets (brief §3 / §12)

- Tools: 19 shipped / 25 cap.
- `tools/list` payload: ≤ 2,500 tokens, measured with a real
  tokenizer at T6. T0 measurement: _pending_.
- Session co-load: torori + neemu inside the ~40-tool
  session ceiling — plan documented at T6.

## Per-tool timeouts (documented, probed at T6)

See `src/server.rs` `tool_specs()` — each tool carries its
`timeout_secs` from birth.

## T6 roll-up (2026-10-27, from gate evidence)

- Tools: **19 shipped / 25 cap**. The binding constraint is
  the payload budget, not the count: any new tool must budget
  its description + schema against the cap before shipping.
- `tools/list` payload: **2,454 tokens** vs the 2,500 cap —
  wire-compact result object, Qwen2.5-Coder BPE (HF
  transformers, primo). First measurement was 2,604 (over);
  descriptions were compressed with contract facts kept, then
  re-measured. Margin: 46 tokens.
- Co-load: torori (19) + neemu (20 per its SPEC) = 39 inside
  the ~40-tool session ceiling — one spare. Relief valve:
  trigger-scoped loading by role group (server, catalogue,
  library, bundles, pairs, voice, sfx, comp).
- Timeouts are documented per tool in `tool_specs()`
  (reads 10 s; validates 30 s; ingest 120–300 s; SFX 60 s;
  comp 300 s; voice generate 900 s) and the fault-injection
  suite proves malformed input returns `isError` on every
  tool — no hangs, no panics, no protocol-error leaks.
- Declared vs detected access: annotations on all 19 tools
  asserted at the wire; write posture declared exactly on
  catalogue transition, library fetch/ingest, bundle ingest,
  voice generate, SFX render, comp assemble — matching the
  dispatch behaviour.
- Contract matrix: raw modern stdio RUN/PASS (8 wire tests);
  fault injection RUN/PASS; Inspector CLI refused by design
  (`-32022` to its legacy `initialize`; fleet-known CLI
  limitation, T0 evidence); rose and phlow NOT RUN against
  torori (fleet-proven WS0 clients; torori-specific sessions
  are first-use acts of the consuming work); neemu-shaped
  consumer RUN (the T3 fixture ingest, TORORI-T3-REPORT.md).
- Silence posture verified by scan at T6: no audio-output
  dependency in Cargo.lock; no playback/sink/autoplay surface
  in source; QC metric-only; generation headless batch.
