---
origin_lane: ntranch
content_label: sfw
evidence: probe logs at primo ~/.local/share/pax/f3-20261104/{baseline,built,after}-probe.txt; landing commit 075209db6faa14ac713e8dfaecac667376a4ac7a on pax/torori-regdiet-20261104
---
# Lesson — a surface diet is per launched profile, not per server

last_verified: 2026-11-04
applies_to: torori 1.0.0 (D-934 profiles, D-954 diet, D-984 audit F3)
disposition:
scope: repo_task
subject: MCP handshake token budgets

A diet record measured on the DEFAULT profile (torori core: 5 tools, 692 cl100k_base tokens over the tools/list array) says nothing about a REGISTERED multi-pack profile: the rose-registered launch `--profile bundles,library,voice` served 17 tools at 2,506 tokens, 6 over the 2,500 budget, while the diet record read as compliant.

Fix pattern that worked: probe every launched profile with the exact registered command (modern server/discover + tools/list, cl100k_base over json.dumps(tools array)), baseline first and reproduce the audit numbers before editing, then trim only description text in the over-budget packs (names, tool count, input schemas byte-identical with descriptions stripped). 10 description trims: 2,506 -> 2,480; default unchanged at 692; full moved 2,911 -> 2,885 by the same shared strings, tool set unchanged.
