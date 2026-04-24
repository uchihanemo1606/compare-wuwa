# WWMI Anchor Pivot Brief for Codex

Status: active working brief for the current project direction.

This file exists to help a fresh Codex account understand the current backend
goal without re-deriving the whole project history from scratch.

Standing instructions still apply first:
- `AGENTS.md`
- `.cursor/rules/*.mdc`

If any statement in this brief conflicts with those standing instructions, the
standing instructions win.

## 1. What this project was trying to do

The original product thesis of `WhashReonator` was:

1. capture rich per-version game evidence,
2. compare versions conservatively,
3. build continuity/history across updates,
4. infer repair guidance for WWMI-backed mods.

That architecture produced substantial backend work in:
- `src/snapshot/mod.rs`
- `src/compare/mod.rs`
- `src/report/mod.rs`
- `src/report_storage/mod.rs`
- `src/inference/mod.rs`
- `src/proposal/mod.rs`

## 2. Where the thesis drifted

After reviewing the public `WWMI-Package` repository and its patch-to-patch
history, the current working conclusion is:

- the real WWMI patch cycle is usually not driven by broad whole-game asset
  lineage drift,
- the most patch-critical WWMI updates are often small edits to shared runtime
  anchors and startup/runtime config,
- many mods "come back to life" after WWMI updates because they depend on the
  same shared runtime hook layer, not because WWMI remaps the whole game.

This means the old "whole-game compare first, infer later" thesis is too broad
to be the primary product focus for the next phase.

It does not make the whole repository useless, but it does mean the project
must pivot to a much narrower runtime-evidence workflow.

## 3. New thesis

The current backend target is:

"Use WWMI / 3DMigoto Frame Analysis dumps as the canonical runtime oracle,
extract the shared WWMI anchors plus mod-derived runtime signals, and produce
review-friendly reports about what changed and what is likely affected."

In short:

- not "analyze the whole game first"
- but "analyze the WWMI runtime contract first"

## 4. Canonical WWMI anchors

These six hashes are the current canonical WWMI anchors that matter for the
pivoted backend.

1. `8c1ee0581cb4f0ec` -> `CharacterMenuBackgroundParticlesVS` -> `vertex_shader`
2. `a24b0fd936f39dcc` -> `UIDrawPS` -> `pixel_shader`
3. `2cc576e7` -> `OutfitTileSideGradientsImage` -> `texture_resource`
4. `ce6a251a` -> `OutfitTileBackgroundImage` -> `texture_resource`
5. `9bf4420c82102011` -> `ShapeKeyLoaderCS` -> `compute_shader`
6. `7a8396180d416117` -> `ShapeKeyMultiplierCS` -> `compute_shader`

Capture profiles:

- `menu_ui` expects anchors 1-4
- `shapekey_runtime` expects anchors 5-6
- `full` expects all 6 anchors

Important:

- do not assume every single dump must contain all six anchors,
- validate against the intended capture profile instead.

## 5. Where these anchors come from

These anchors come from the game runtime, not from a `.pak` parser and not from
an offline asset database.

The public WWMI package itself documents the workflow:
- hunting mode
- copying shader/buffer hashes
- `F8` frame dump generation

Relevant local evidence:
- `_upstream/WWMI-Package/WWMI/Core/WWMI/Notifications/HuntingModeGuide.md`
- `_upstream/WWMI-Package/WWMI/Core/WWMI/WWMI-Utilities.ini`
- `_upstream/WWMI-Package/WWMI/Core/WWMI/WuWa-Model-Importer.ini`

This is consistent with repository ADR-001:
- `docs/adr-001-no-pak-parser.md`

That ADR already established that Frame Analysis is the canonical oracle for
runtime hashes and that a Rust `.pak` parser is out of scope.

## 6. What "success" means now

For the next phase, success is narrow and concrete.

The backend succeeds if it can:

1. read a WWMI / 3DMigoto `F8` Frame Analysis dump,
2. extract the six canonical anchors by expected kind plus hash,
3. validate coverage by capture profile,
4. export a machine-readable report listing found and missing anchors,
5. reuse that result later when joining with mod-derived runtime signals.

This phase does NOT need to:

- solve whole-game compare,
- infer every mod fix automatically,
- build a GUI workflow,
- reverse-engineer `.pak` files,
- re-center the product around broad asset lineage analysis.

## 7. Code that already points in the right direction

Reuse existing code before adding new architecture.

Most relevant files:

- `src/ingest/frame_analysis.rs`
- `src/pipeline/mod.rs`
- `src/cli/mod.rs`
- `src/wwmi/dependency.rs`
- `tests/frame_analysis_wwmi_anchors.rs`
- `tests/frame_analysis_ingest.rs`
- `tests/frame_analysis_compare.rs`
- `tests/frame_analysis_identity.rs`
- `tests/frame_analysis_archive_dump.rs`

Current useful facts:

- `src/ingest/frame_analysis.rs` already parses and surfaces:
  - `vertex_shader`
  - `pixel_shader`
  - `compute_shader`
  - `texture_resource`
- `tests/frame_analysis_wwmi_anchors.rs` already contains a sample log with all
  six canonical anchors.

This means the next slice should be additive, not a rewrite.

## 8. What to stop doing

Do not spend the next phase on:

- broad new work in `snapshot -> compare -> continuity -> inference -> proposal`
  unless it directly supports the anchor pivot,
- restoring a `.pak` branch,
- generic GUI polish,
- mod authoring features,
- speculative whole-game data models that are not needed to extract or validate
  WWMI runtime anchors.

## 9. Recommended next backend slice

Build a small backend feature that:

1. parses a dump directory,
2. extracts canonical WWMI anchors,
3. validates capture profile coverage,
4. exports a stable JSON report.

Good command names:

- `extract-wwmi-anchors`
- `audit-wwmi-anchors`

Suggested report fields:

- `schema_version`
- `capture_profile`
- `source_dump_dir`
- `source_log_path`
- `found_anchors`
- `missing_anchors`
- `unexpected_anchor_candidates`
- `success`
- `notes`

The next follow-up phase after exact-match extraction is documented in:
- `docs/phase3a-wwmi-anchor-discovery.md`

## 10. Reporting language

User-facing updates and final reports must be in Vietnamese unless the user
explicitly requests another language.

Keep code symbols, file paths, commands, and literal technical identifiers in
their original language.

## 11. Ready handoff prompt

Use the prompt below when handing the next coding task to Codex.

```text
You are working in the Rust repository `d:\projectIT\WhashReonator`.

Before making changes, read these files first:
- `AGENTS.md`
- `.cursor/rules/codex-core-mission.mdc`
- `.cursor/rules/codex-language-reporting.mdc`
- `docs/adr-001-no-pak-parser.md`
- `docs/codex-wwmi-anchor-pivot.md`

Use the `backend-phase-executor` skill.

Important reporting rule:
- Work in English internally if you want, but report progress updates, findings,
  and the final implementation summary in Vietnamese.
- Keep code symbols, file paths, commands, and literal technical values exactly
  as they are.

Current project context:
- The repository originally invested heavily in `snapshot -> compare ->
  continuity -> inference -> proposal`.
- After reviewing public WWMI patch history, the current working conclusion is
  that patch-critical WWMI updates usually touch a small shared runtime hook
  layer, not broad whole-game asset lineage.
- Therefore the current backend priority is to pivot toward runtime anchor
  extraction from WWMI / 3DMigoto Frame Analysis dumps.

Canonical WWMI anchors:
1. `8c1ee0581cb4f0ec` -> `CharacterMenuBackgroundParticlesVS` -> `vertex_shader`
2. `a24b0fd936f39dcc` -> `UIDrawPS` -> `pixel_shader`
3. `2cc576e7` -> `OutfitTileSideGradientsImage` -> `texture_resource`
4. `ce6a251a` -> `OutfitTileBackgroundImage` -> `texture_resource`
5. `9bf4420c82102011` -> `ShapeKeyLoaderCS` -> `compute_shader`
6. `7a8396180d416117` -> `ShapeKeyMultiplierCS` -> `compute_shader`

Capture profiles:
- `menu_ui` expects anchors 1-4
- `shapekey_runtime` expects anchors 5-6
- `full` expects all 6 anchors

What success means for the next slice:
- read a WWMI / 3DMigoto `F8` dump,
- extract these anchors by expected kind plus hash,
- validate capture coverage by profile,
- export a stable machine-readable report,
- do this with the smallest additive backend slice possible.

Do not drift into:
- `.pak` parsing,
- broad whole-game compare work unless directly needed for anchor extraction,
- unrelated GUI changes,
- mod authoring features.

Most relevant existing files:
- `src/ingest/frame_analysis.rs`
- `src/cli/mod.rs`
- `src/pipeline/mod.rs`
- `src/wwmi/dependency.rs`
- `tests/frame_analysis_wwmi_anchors.rs`
- `tests/frame_analysis_ingest.rs`

Implementation constraints:
- Reuse existing `frame_analysis` parsing and inventory-building code.
- Keep new or updated tests in `tests/*.rs`.
- Preserve unrelated local changes.
- Use approved artifact paths only; never write into `src/`, `tests/`, the real
  game root, or real mod roots.

Your task:
Implement a backend-only WWMI anchor extraction/reporting slice around the six
canonical anchors above, with clear profile-aware validation and focused tests.
```
