# Roadmap — torori (mcp) — v0.0 → v1.0
<!-- bloch:roadmap schema=1 kind=mcp lane=general baseline=0.0.1 target=1.0.0 -->

How to work this roadmap (any agent): run `bloch task start R-xxx --agent <you>` before working a task, `bloch task done R-xxx` when you believe it is complete — done runs every `verify:` command and flips the task green only if all pass; there is no override. `bloch task fail R-xxx --error "..."` records a failure. Fixes learned the hard way land in `.ai/memory/lessons.md` via the inbox/promote flow and are surfaced in every HANDOFF.

Curated 2026-10-28 (D-313 Phase 0 — NTRanch four → 1.0 program): the template frame below is torori's real 1.0 path per the fleet gap list (G-T2 version+docs+re-gate, G-T3 consumer sessions, publish 1.0.0). Sound gates are Owner-deferred in Phase 7 (D-314) — they are not build items.

## Phase 1 — Baseline truth
- [x] R-101 Build is green from a clean checkout
  - verify: cargo build
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:48:30Z
- [x] R-102 Existing tests pass at the baseline
  - verify: cargo test
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:48:36Z
- [x] R-103 Repo identity, baseline version, and this roadmap are recorded
  - verify: test -f .ai/roadmap.md
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:48:36Z

## Phase 2 — Core complete
- [x] R-201 Tool surface is contracted: every tool has a complete schema and annotations from birth
  - verify: cargo test
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:48:42Z
- [x] R-202 Tool count stays within the declared budget (<= 25 tools)
  - verify: cargo test
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:48:48Z
- [x] R-203 Typed error and refusal behaviour is implemented in server code, never left to client hints
  - verify: cargo test
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:48:55Z

## Phase 3 — Tests and gates
- [x] R-301 Validation tests cover the happy path end to end (half the test split)
  - verify: cargo test
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:49:01Z
- [x] R-302 Adversarial tests cover malformed, oversized, and hostile inputs (half the test split)
  - verify: cargo test
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:49:07Z
- [x] R-303 Refusal and typed-error behaviour is covered by tests, including oversized payloads
  - verify: cargo test
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:49:13Z

## Phase 4 — Hardening
- [x] R-401 MCP Inspector conformance pass is recorded as evidence
  - verify: cargo test
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:49:22Z
- [x] R-402 Client matrix is verified with at least two real clients
  - verify: cargo test
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:49:28Z
- [x] R-403 Declared-vs-detected access (EFLO) check passes: no tool exceeds its annotations
  - verify: cargo clippy -- -D warnings
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:49:28Z
- [x] R-404 Registry/release packaging builds from a clean checkout
  - verify: cargo build
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:49:28Z

## Phase 5 — Docs
- [x] R-501 README documents install, usage, and verification
  - verify: test -f README.md
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:49:28Z
- [x] R-502 CHANGELOG records every user-facing change
  - verify: test -f CHANGELOG.md
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:56:16Z
- [x] R-503 Tool reference documents every tool, schema, annotation, and error kind
  - verify: test -f README.md
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:49:28Z
- [x] R-504 README/COMPLIANCE currency against the landed 19-tool surface (G-T2): COMPLIANCE written current, deferred list carried into the 1.0.0 docs
  - verify: test -f COMPLIANCE.md
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:56:16Z

## Phase 6 — Release v1
- [x] R-601 Version is bumped to 1.0.0 and the changelog records the release (G-T2; re-gate on the 1.0.0 tree with freeze-artefact checksums e0df698d… / f423f883… unchanged)
  - verify: grep -q 'version = "1.0.0"' Cargo.toml
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:56:16Z
- [x] R-602 Clean-environment install and run is verified
  - verify: cargo build
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:58:47Z
- [x] R-603 Release artefacts are built from a clean checkout and checksummed
  - verify: cargo build
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:58:47Z
- [x] R-604 Consumer sessions filed as D-116 utilization evidence (G-T3): first rose/phlow client sessions + one game-shaped production session; failures fixed in place or recorded
  - verify: test -f UTILIZATION.md
  - evidence: 1 verify commands, all exit 0 at 2026-10-29T04:57:52Z
- [ ] R-605 torori 1.0.0 published to crates.io under the NTRanch account
  - verify: curl -sf -H "User-Agent: bloch-roadmap" https://crates.io/api/v1/crates/torori/1.0.0 -o /dev/null
  - evidence:

## Phase 7 — Owner-deferred (D-314: tabled by the Owner 2026-10-28 — NOT build items)
- [ ] R-701 T4 voice takes ×2 graded by the Owner's ear (sound gate; tabled for tonight per D-314, resumes on the Owner's word — grading filed as evidence to flip)
  - verify: test -f .ai/memory/evidence/t4-takes-graded.md
  - evidence:
- [ ] R-702 VoiceIdentity registry filled by Owner canon acts (empty by design until then; canon acts filed as evidence to flip)
  - verify: test -f .ai/memory/evidence/voice-identity-canon.md
  - evidence:
