# Compliance — torori 1.0.0

The compliance record for torori, written current against the
landed 19-tool surface at 1.0.0. Every statement below is
traceable to the tree (source, `README.md`, `EFLO.md`) or to
the T0–T6 build record it landed with.

## Identity and protocol posture

- Crate `torori`, version 1.0.0, licence Apache-2.0. Studio
  attribution: Nice Thoughts Ranch (NTRanch).
- Built on rmcp `=3.8.0` (the fleet pin, D-113), modern MCP
  era only (2026-07-28). Sessions open with `server/discover`;
  every request carries its protocol version in `_meta`.
- Legacy contact is refused in code, not by client hints: the
  stdio era guard (`src/guard.rs`) answers any `initialize`
  with the typed `-32022` unsupported-version error naming the
  supported versions, and the process stays up. The guard also
  owns the framing contract: malformed JSON → `-32700`,
  non-object request → `-32600`, unknown method → `-32601`,
  4 MiB line bound, notifications dropped and never executed.
- The handler pins `supported_protocol_versions` to
  2026-07-28, so `server/discover` advertises exactly one
  version.

## Tool surface (19 tools)

Groups for trigger-scoped loading: server, catalogue, library,
bundles, pairs, voice, sfx, comp.

- Server (read): `torori_status`.
- Catalogue: `torori_catalogue_search`, `torori_catalogue_get`,
  `torori_catalogue_stats` (read); `torori_catalogue_transition`
  (write).
- Library: `torori_library_search`, `torori_library_manifest`
  (read); `torori_library_fetch` (write),
  `torori_library_ingest` (write, idempotent).
- Bundles and pairs: `torori_bundle_validate`,
  `torori_bundle_list`, `torori_pair_validate`,
  `torori_pair_list` (read); `torori_bundle_ingest` (write,
  idempotent).
- Voice, SFX, comp: `torori_voice_list`, `torori_voice_qc`
  (read); `torori_voice_generate`, `torori_sfx_render`,
  `torori_comp_assemble` (write).

Every tool shipped with a complete schema and behaviour
annotations from birth (readOnly / destructive / idempotent
hints). Declared access matches detected access: write posture
is declared exactly on catalogue transition, library
fetch/ingest, bundle ingest, voice generate, SFX render, and
comp assemble (EFLO record, T6 roll-up). Per-tool timeouts are
documented in `src/server.rs` `tool_specs()` (reads 10 s;
validates 30 s; ingest 120–300 s; SFX 60 s; comp 300 s; voice
generate 900 s); a timeout returns an `isError` result with
code `timeout` — never a protocol error, never a hang. Domain
failures return as `isError` results carrying the typed
refusal payload; unknown tools are protocol errors (`-32602`).
`tools/list` measures 2,454 tokens against the 2,500 cap
(real tokenizer; `EFLO.md`).

## Safety posture — decide() in server code

- **Licence-state gate.** A library row's fetchability is a
  pure `decide()` over its licence state (`src/library.rs`):
  only CLEARED rows fetch; UNDETERMINED and NOT CLEARED rows
  refuse, and a file whose sha256 has drifted from the INDEX
  refuses. Ingest takes licence state only from each file's
  per-file capture sidecar — no capture means UNDETERMINED,
  never rounded up; NC/ND means NOT CLEARED. There is no bulk
  export, by design.
- **Catalogue governance.** Status transitions are governed
  in server code: GENERATED requires asset refs plus §5.2
  manifest refs; leaving GENERATED requires an ear verdict
  (`{verdict, admitted_by}`) — the Owner's gate event. The
  provenance class is never overwritten.
- **Bundle ingest.** Quarantine-copy, then the full validate
  battery (schema, hashes, provenance, tier stamps,
  target_voice, pairs V1–V8) in a fixed order — the first
  failure refuses with its typed code. Partial ingest never
  happens; unvalidated bundles are never visible to the
  generation tools; ingest is idempotent on content hash.
- **Comp lane.** The §4.3 numeric ship gate decides in code:
  NO-SHIP returns the untreated comp with its failing numbers,
  never a silently cleaned asset. Generated layers into a
  library-derived base refuse (laundering guard); the SFX
  lane is BIT-EXACT only and refuses any other determinism
  class rather than substituting a lane.

## Standing law

- **Audio silence rule**: this codebase contains no audio
  playback path. Generation and QC are file metrics only;
  every output is a file plus its metrics. Verified by scan
  at T6: no audio-output dependency in `Cargo.lock`, no
  playback/sink/autoplay surface in source. Ear gates are the
  Owner's acts on delivered files.
- **Rights law**: no real-person voice likeness, ever.
  OpenNSFW pack clips are comping material only — never a
  prompt, prefix, reference, fine-tune set, or conversion
  source/target for any AI lane. Generated material is never
  comped into a pack-derived asset. Every asset carries its
  §5.2 manifest, which is its rights record.
- **Determinism contract**: every asset declares BIT-EXACT,
  SEED-REPRODUCIBLE, or OUTPUT-UNIQUE and is never described
  above its class.

## Freeze artefacts

The torori↔neemu transport contract, frozen at T3 and
unchanged at 1.0.0 (sha256):

- Conditioning schema v1, `data/schemas/conditioning-v1.json`:
  `e0df698dd61c9ad36b9832c8d9e2a79d2d86ea19292e5893d9cf621a1e0e6610`
- Cue-bundle schema v1, `data/schemas/bundle-v1.json`:
  `f423f88305d46b41cc2c1863a5e32049d7f5e62a8ae27270117ac5c737177138`

## Deferred and open (carried from the build record)

- The two T4 trial voice takes are UNGRADED (Owner ear act).
- The VoiceIdentity registry ships empty by design; canon
  heroine registrations and locked references are Owner
  canon acts.
- The OpenNSFW SFX pack (B-5) is not ingested; its ingest
  must capture the hub link list live.
- Nonverbal graded arcs are UNPROVEN on every lane; their
  catalogue entries stay GAP.
- Measurement caveats on record: QC loudness is the in-tree
  estimator (`lufs_integrated_est`, no K-weighting), labelled
  as such; the F0 tracker flags its own instability on
  breathy material.
