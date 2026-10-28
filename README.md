# torori

The NTRanch estate sound MCP (rmcp, modern MCP era only —
2026-07-28). Four functions:

- **catalogue** — serving over the 114-entry TORORI catalogue
  (kinks, acts, reactions) with its status state machine;
- **library** — the OpenNSFW library lane: per-file licence
  states, an ingest-time capture discipline, and a fetch that
  copies one cleared file out with its attribution manifest entry
  in the same operation. No bulk export exists, by design;
- **pairs + bundles** — GAP5 TriggerPair validation (rules
  R1–R7, battery V1–V8) and file-bundle cue ingest (bundles are
  the only torori↔neemu transport; there is no server coupling);
- **generation** — the Qwen3-TTS voice lane behind the
  VoiceIdentity registry, deterministic DSP SFX instruments, and
  the Route C comp lane under the §4.3 numeric ship gate.

## Standing law (enforced in code)

- **Audio silence rule**: this codebase contains no audio
  playback path. Generation and QC are file metrics only; every
  output is a file plus its metrics. Ear gates are the Owner's
  acts on delivered files, recorded as verdict records.
- **Rights law**: no real-person voice likeness, ever; OpenNSFW
  pack clips are comping material only — never a prompt, prefix,
  reference, fine-tune set, or conversion source/target for any
  AI lane; generated material is never comped into a pack-derived
  asset; every asset carries its §5.2 manifest, which is its
  rights record.
- **Determinism contract**: every asset declares BIT-EXACT,
  SEED-REPRODUCIBLE, or OUTPUT-UNIQUE and is never described
  above its class.

## Running

`torori [--data <dir>] [--root <dir>]` — stdio. `--data` is the
shipped data tree (`data/` in this repo); `--root` is the
writable state tree (default `~/.local/share/torori`).

Built under the estate fleet standardization (D-113): rmcp
`=3.8.0`, modern era only; legacy `initialize` contact is
refused at the stdio era guard with `-32022` and the supported
versions named.
