# Utilization — torori 1.0.0 (D-116 consumer-session evidence)

Two real scripted stdio sessions run against the built 1.0.0
binary on primo (2026-10-28), modern era only (2026-07-28):
sessions open with `server/discover`, every request carries
its protocol version in `_meta`. Both sessions are silent —
files and metrics only; no audio was played anywhere (the
audio silence rule). Full machine transcripts were captured
at session time; the frames below are those transcripts
(tools/list is summarized to names + annotations; every other
frame is verbatim, long text payloads trimmed at 1,200
characters and marked by their length in the raw capture).

## Session A — MCP-Inspector-style (discover, list, representative calls)

Driven the way the MCP Inspector drives a server: open,
discover the surface, call representative tools. The legacy
`initialize` probe is the Inspector CLI's known shape against
a modern-only server — the designed `-32022` refusal is the
expected answer (fleet-known CLI limitation, recorded at T0),
and the same session continues serving modern frames after it.

Verdicts: discover advertises exactly `2026-07-28` and
serverInfo `torori 1.0.0`; tools/list returns the 19 tools;
`torori_status` reports 114 catalogue entries, 14 pairs, 0
voice identities (the registry ships empty by design);
catalogue stats reproduce the build record exactly (Kink 57 /
Act 34 / Reaction 23; GAP 63 / PLANNED 40 / MEASURED-ONLY 7 /
HAVE-LIBRARY 4); an unknown tool is refused as a protocol
error `-32602`.

### server/discover (modern session open)
Request: `{"id": 1, "jsonrpc": "2.0", "method": "server/discover", "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "inspector-style", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"resultType": "complete", "supportedVersions": ["2026-07-28"], "capabilities": {"tools": {}}, "ttlMs": 0, "cacheScope": "private", "_meta": {"io.modelcontextprotocol/serverInfo": {"name": "torori", "version": "1.0.0"}}}`

### legacy initialize probe (Inspector CLI shape) — designed refusal
Request: `{"id": 90, "jsonrpc": "2.0", "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "mcp-inspector-cli", "version": "0"}}}`
Response: `{"error":{"code":-32022,"data":{"requested":"2025-06-18","supported":["2026-07-28"]},"message":"Unsupported protocol version"},"id":90,"jsonrpc":"2.0"}`

### tools/list
Request: `{"id": 3, "jsonrpc": "2.0", "method": "tools/list", "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "inspector-style", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: 19 tools — torori_status (readOnly=True, idempotent=True), torori_catalogue_search (readOnly=True, idempotent=True), torori_catalogue_get (readOnly=True, idempotent=True), torori_catalogue_stats (readOnly=True, idempotent=True), torori_catalogue_transition (readOnly=False, idempotent=False), torori_library_search (readOnly=True, idempotent=True), torori_library_fetch (readOnly=False, idempotent=False), torori_library_manifest (readOnly=True, idempotent=True), torori_library_ingest (readOnly=False, idempotent=True), torori_bundle_validate (readOnly=True, idempotent=True), torori_bundle_ingest (readOnly=False, idempotent=True), torori_bundle_list (readOnly=True, idempotent=True), torori_pair_validate (readOnly=True, idempotent=True), torori_pair_list (readOnly=True, idempotent=True), torori_voice_list (readOnly=True, idempotent=True), torori_voice_generate (readOnly=False, idempotent=False), torori_sfx_render (readOnly=False, idempotent=False), torori_comp_assemble (readOnly=False, idempotent=False), torori_voice_qc (readOnly=True, idempotent=True). (Full schemas on the wire; payload measured 2,454 tokens at T6 against the 2,500 cap — see EFLO.md.)

### tools/call torori_status
Request: `{"id": 4, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_status", "arguments": {}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "inspector-style", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"counts":{"bundles_ingested":0,"catalogue_entries":114,"library_rows":0,"pairs":14,"voice_identities":0},"lanes":{"dsp_sfx":"available","qwen_tts":"driver-unconfigured","route_c_comp":"available"},"name":"torori","protocol":"2026-07-28","silence_rule":"enforced: no playback path exists in this codebase; outputs are files + metrics only","version":"1.0.0"}`

### tools/call torori_catalogue_stats
Request: `{"id": 5, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_catalogue_stats", "arguments": {}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "inspector-style", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"by_kind":{"ACT":34,"KINK":57,"REACTION":23},"by_status":{"GAP":63,"HAVE-LIBRARY":4,"MEASURED-ONLY":7,"PLANNED":40},"total":114,"with_form":8,"with_pair":8}`

### tools/call torori_catalogue_search kind=REACTION limit=3
Request: `{"id": 6, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_catalogue_search", "arguments": {"kind": "REACTION", "limit": 3}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "inspector-style", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"items":[{"assets":"recovery breath loops at 2–3 decay grades, soft hums, trailing fragments bank.","brief":null,"coupling":"terminal phase of every arc; owned by the phase machine (RUNTIME §4.1).","description":"Post-climax physiological settle: decelerating breath, softening vocal register, graded recovery fragments rather than silence.","ear_admission":null,"form":null,"generated":null,"kind":"REACTION","origin_status":null,"pair":null,"slug":"reaction-afterglow-recovery","source":"PERFORMANCE.md §5.1/§5.3 (recovery graded as distinct states).","status":"PLANNED","status_note":null,"title":"Afterglow / recovery"},{"assets":"the peak vocal register (see Climax / orgasm event) — the face and the peak voice are one coupled state, never shipped separately.","brief":null,"coupling":"fires at threshold crossing; pairs with Eye roll at climax and Trembling.","description":"Peak facial state: half-lidded upturned eyes, open mouth, luminous heart pupils; the estate's named climax-expression standard.","ear_admission":null,"form":null,"generated":null,"kind":"REACTION","origin_status":null,"pair":null,"slug":"reaction-ahegao-torogao-face","source":"STUDY.md (post 9228588 = the named aheg`

### tools/call torori_pair_list limit=3
Request: `{"id": 7, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_pair_list", "arguments": {"limit": 3}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "inspector-style", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"items":[{"consumer_class":"Fiction","cue_kind":"Snap","cue_sha256":"241e25052cba6dded37d22a8058aa20db87b47ed57c6ee1c446b4dcb89e7d616","origin":"data/pairs","pair_id":"pair.torori.kit.drop-snap","program_id":"torori.kit","release":"embedded","subject_form":"Single"},{"consumer_class":"Fiction","cue_kind":"SpokenPhrase","cue_sha256":"3c023bde3be50c8cd835817e3fc7b37fa9a052dd61e337a28dcb6a2221e80815","origin":"data/pairs","pair_id":"pair.torori.kit.hypnosis-frame","program_id":"torori.kit","release":"shared:release.torori.kit.reset","subject_form":"Single"},{"consumer_class":"Fiction","cue_kind":"SpokenWord","cue_sha256":"1c17ae0a5e938cc885953dd5bee5070a4211ebb7702cbd0965731d2f5e6abb11","origin":"data/pairs","pair_id":"pair.torori.kit.countdown","program_id":"torori.kit","release":"shared:release.torori.kit.reset","subject_form":"Single"}],"limit":3,"offset":0,"returned":3,"total":14,"truncated":true}`

### tools/call torori_voice_list (registry ships empty by design)
Request: `{"id": 8, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_voice_list", "arguments": {}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "inspector-style", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"items":[],"limit":50,"offset":0,"returned":0,"total":0,"truncated":false}`

### tools/call unknown tool (typed protocol refusal)
Request: `{"id": 9, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_nope", "arguments": {}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "inspector-style", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"code": -32602, "message": "unknown tool: torori_nope"}`

## Session B — game-shaped production session (cue-bundle flow + SFX)

The flow a game drives at production time: look up the cue
need in the catalogue, take a producer's cue bundle through
validate → ingest → list, and render a deterministic SFX cue.
The bundle is the T3 neemu-shaped fixture shape (one cue,
conditioning-v1 record, one WAV asset with a complete §5.2
manifest), produced for this session with torori's own types;
the server ran against a game-shaped data tree (shipped data
plus one anchored fixture VoiceIdentity).

Verdicts: validate returns `valid: true` (content hash
`3f3fef9a…`); ingest promotes the bundle; a second ingest of
the same bundle answers `idempotent: true` with the same
content hash and stored dir; bundle_list shows the ingested
bundle; two `torori_sfx_render` calls with identical
parameters (impact, seed 7, 0.5 s, BIT-EXACT) return the same
asset id (`sfx.impact-ef234d29-7`) with identical QC metrics —
the determinism contract observed at the wire; closing status
shows `bundles_ingested: 1` and version 1.0.0. The voice lane
reports `driver-unconfigured` in this environment, so no
voice generation was attempted in these sessions — recorded,
not worked around.

### server/discover
Request: `{"id": 1, "jsonrpc": "2.0", "method": "server/discover", "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "game-production", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"resultType": "complete", "supportedVersions": ["2026-07-28"], "capabilities": {"tools": {}}, "ttlMs": 0, "cacheScope": "private", "_meta": {"io.modelcontextprotocol/serverInfo": {"name": "torori", "version": "1.0.0"}}}`

### game looks up its cue need: torori_catalogue_search query=breath limit=2
Request: `{"id": 2, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_catalogue_search", "arguments": {"query": "breath", "limit": 2}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "game-production", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"items":[{"assets":"recovery breath loops at 2–3 decay grades, soft hums, trailing fragments bank.","brief":null,"coupling":"terminal phase of every arc; owned by the phase machine (RUNTIME §4.1).","description":"Post-climax physiological settle: decelerating breath, softening vocal register, graded recovery fragments rather than silence.","ear_admission":null,"form":null,"generated":null,"kind":"REACTION","origin_status":null,"pair":null,"slug":"reaction-afterglow-recovery","source":"PERFORMANCE.md §5.1/§5.3 (recovery graded as distinct states).","status":"PLANNED","status_note":null,"title":"Afterglow / recovery"},{"assets":"clean single-cycle breath loops (loopable, noise-free), heavy-breath grades 1–3, breath-rate as a continuous parameter slaved to the reservoir integrator (RUNTIME §9.2.2).","brief":null,"coupling":"underlies every vocal entry; breath brackets are the splice-hiding points for comping (PERFORMANCE AC-3).","description":"The breath layer as a first-class signal: paced cycles under induction, heavy cycles under effort/arousal.","ear_admission":null,"form":null,"generated":null,"kind":"REACTION","origin_status":null,"pair":null,"slug":"reaction-breathing-paced-an`

### torori_bundle_validate on the producer bundle (no ingest)
Request: `{"id": 3, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_bundle_validate", "arguments": {"bundle_dir": "/tmp/game-session/bundle-src"}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "game-production", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"assets":1,"bundle_id":"bundle.fixture.neemu-shaped","content_hash":"3f3fef9af55b6f65987c197cf9f454057f4477f3af12039b5d79fcf92f4156a1","cues":1,"pairs":0,"valid":true}`

### torori_bundle_ingest (quarantine -> validate -> promote)
Request: `{"id": 4, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_bundle_ingest", "arguments": {"bundle_dir": "/tmp/game-session/bundle-src"}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "game-production", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"bundle_id":"bundle.fixture.neemu-shaped","content_hash":"3f3fef9af55b6f65987c197cf9f454057f4477f3af12039b5d79fcf92f4156a1","cues":1,"idempotent":false,"pairs":0,"stored_dir":"/tmp/game-session/state/bundles/ingested/bundle.fixture.neemu-shaped@3f3fef9af55b"}`

### torori_bundle_ingest again (idempotent on content hash)
Request: `{"id": 5, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_bundle_ingest", "arguments": {"bundle_dir": "/tmp/game-session/bundle-src"}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "game-production", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"bundle_id":"bundle.fixture.neemu-shaped","content_hash":"3f3fef9af55b6f65987c197cf9f454057f4477f3af12039b5d79fcf92f4156a1","cues":1,"idempotent":true,"pairs":0,"stored_dir":"/tmp/game-session/state/bundles/ingested/bundle.fixture.neemu-shaped@3f3fef9af55b"}`

### torori_bundle_list
Request: `{"id": 6, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_bundle_list", "arguments": {}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "game-production", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"items":[{"bundle_id":"bundle.fixture.neemu-shaped","content_hash":"3f3fef9af55b6f65987c197cf9f454057f4477f3af12039b5d79fcf92f4156a1","cues":1,"pairs":0,"producer":"neemu-fixture@0.0.0+test","stored_dir":"/tmp/game-session/state/bundles/ingested/bundle.fixture.neemu-shaped@3f3fef9af55b"}],"limit":50,"offset":0,"returned":1,"total":1,"truncated":false}`

### torori_sfx_render impact seed=7 (deterministic DSP, file+metrics only)
Request: `{"id": 7, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_sfx_render", "arguments": {"instrument": "impact", "seed": 7, "duration_sec": 0.5, "determinism_class": "BIT-EXACT"}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "game-production", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"asset_id":"sfx.impact-ef234d29-7","determinism_class":"BIT-EXACT","file":"/tmp/game-session/state/assets/sfx.impact-ef234d29-7/render.wav","qc":{"breath_bracket":null,"channels":1,"clipped_samples":0,"crest_factor_db":14.636,"dc_offset":-0.0,"duration_delta_sec":0.0,"duration_sec":0.5,"envelope_dynamic_range_db":12.318,"f0_median_hz":59.85,"f0_range_semitones":19.346,"f0_track_stability":0.833,"floor_flatness":0.0,"lufs_integrated_est":-20.519,"noise_floor_dbfs":-31.999,"onset_offset_ms":null,"sample_rate":48000,"true_peak_dbtp_est":-4.779,"voiced_fraction":0.625,"zero_run_max_ms":0.0},"sha256":"ef234d29b0b6f5fe926d391d69a1cb6a509f82f6e0c3b1d5b5d72d1e5c32fdc1"}`

### torori_sfx_render impact seed=7 again (bit-exact determinism check)
Request: `{"id": 8, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_sfx_render", "arguments": {"instrument": "impact", "seed": 7, "duration_sec": 0.5, "determinism_class": "BIT-EXACT"}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "game-production", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"asset_id":"sfx.impact-ef234d29-7","determinism_class":"BIT-EXACT","file":"/tmp/game-session/state/assets/sfx.impact-ef234d29-7/render.wav","qc":{"breath_bracket":null,"channels":1,"clipped_samples":0,"crest_factor_db":14.636,"dc_offset":-0.0,"duration_delta_sec":0.0,"duration_sec":0.5,"envelope_dynamic_range_db":12.318,"f0_median_hz":59.85,"f0_range_semitones":19.346,"f0_track_stability":0.833,"floor_flatness":0.0,"lufs_integrated_est":-20.519,"noise_floor_dbfs":-31.999,"onset_offset_ms":null,"sample_rate":48000,"true_peak_dbtp_est":-4.779,"voiced_fraction":0.625,"zero_run_max_ms":0.0},"sha256":"ef234d29b0b6f5fe926d391d69a1cb6a509f82f6e0c3b1d5b5d72d1e5c32fdc1"}`

### torori_status (post-ingest counts)
Request: `{"id": 9, "jsonrpc": "2.0", "method": "tools/call", "params": {"name": "torori_status", "arguments": {}, "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientInfo": {"name": "game-production", "version": "1.0.0"}, "io.modelcontextprotocol/clientCapabilities": {}}}}`
Response: `{"counts":{"bundles_ingested":1,"catalogue_entries":114,"library_rows":0,"pairs":14,"voice_identities":1},"lanes":{"dsp_sfx":"available","qwen_tts":"driver-unconfigured","route_c_comp":"available"},"name":"torori","protocol":"2026-07-28","silence_rule":"enforced: no playback path exists in this codebase; outputs are files + metrics only","version":"1.0.0"}`

## Failures and gaps

None in either session: every request was answered, no
request timed out, no refusal was unexpected. The two known
non-session items stand as recorded in COMPLIANCE.md: the
Inspector CLI's legacy-only open (designed refusal above)
and the unconfigured Qwen3-TTS driver in this environment.
