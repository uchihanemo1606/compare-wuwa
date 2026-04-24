# Phase 3A — WWMI anchor discovery for unknown game versions

Status: proposed next backend slice after exact-match anchor extraction.

This document assumes the reader has already read:
- `AGENTS.md`
- `.cursor/rules/codex-core-mission.mdc`
- `.cursor/rules/codex-language-reporting.mdc`
- `docs/adr-001-no-pak-parser.md`
- `docs/codex-wwmi-anchor-pivot.md`

## 1. Goal

The exact-match phase is now done:
- the backend can parse a WWMI / 3DMigoto `F8` dump,
- it can validate known canonical WWMI anchors,
- it can distinguish capture profiles such as `menu_ui` and
  `shapekey_runtime`.

The next problem is different:

**When a new game version arrives (for example `3.3`), the project must not
pretend it already knows the new hashes.**

Instead, it must:

1. treat the six WWMI anchors as **logical anchors**,
2. use a fresh dump as the source of truth,
3. surface plausible replacement candidates when old known hashes are missing,
4. help a reviewer decide which new runtime hash most likely replaces each
   logical anchor.

## 2. What is already verified

Current exact-match anchor extraction lives in:
- `src/wwmi/anchors.rs`
- `src/cli/mod.rs`
- `src/pipeline/mod.rs`
- `tests/wwmi_anchor_mode.rs`

The canonical anchors are:

1. `8c1ee0581cb4f0ec` -> `CharacterMenuBackgroundParticlesVS` -> `vertex_shader`
2. `a24b0fd936f39dcc` -> `UIDrawPS` -> `pixel_shader`
3. `2cc576e7` -> `OutfitTileSideGradientsImage` -> `texture_resource`
4. `ce6a251a` -> `OutfitTileBackgroundImage` -> `texture_resource`
5. `9bf4420c82102011` -> `ShapeKeyLoaderCS` -> `compute_shader`
6. `7a8396180d416117` -> `ShapeKeyMultiplierCS` -> `compute_shader`

Real-dump verification already showed:
- two real dumps under `D:\mod\WWMI\FrameAnalysis-2026-04-22-210517` and
  `D:\mod\WWMI\FrameAnalysis-2026-04-22-210709` both pass
  `shapekey_runtime`,
- both of those dumps fail `full` because they are not `menu_ui` captures,
- this proves profile-aware exact matching is working.

## 3. The key product truth

The project cannot infer new WWMI hashes from the game version string alone.

For a future game version like `3.3`, the backend must not do this:
- `3.3 -> hash X`

That would be fabricated.

The stable entities are the **logical anchors**:
- `CharacterMenuBackgroundParticlesVS`
- `UIDrawPS`
- `OutfitTileSideGradientsImage`
- `OutfitTileBackgroundImage`
- `ShapeKeyLoaderCS`
- `ShapeKeyMultiplierCS`

The unstable entity is the **current runtime hash** attached to that logical
anchor in a fresh dump.

Therefore the next phase is a **discovery** phase, not a static lookup phase.

## 4. Recommended smallest slice

Implement the smallest additive backend slice that upgrades the project from
"known-hash validator" to "missing-hash discovery assistant".

Preferred shape:

- keep the current `extract-wwmi-anchors` command working,
- extend the report with replacement candidate data for missing anchors,
- only add a separate command if extending the current command becomes awkward.

Suggested command options if extension is needed:

```bash
cargo run --bin whashreonator -- discover-wwmi-anchor-candidates \
  --dump-dir D:\mod\WWMI\FrameAnalysis-... \
  --capture-profile shapekey-runtime \
  --output out/anchor-candidates.json
```

## 5. Deliverable

The deliverable for this phase is a machine-readable discovery report that
answers:

1. which logical anchors were matched exactly,
2. which logical anchors are still missing,
3. for every missing logical anchor, which observed records are the best
   candidate replacements,
4. why those candidates were ranked highly.

The backend does NOT need to automatically bless or promote a candidate to a
new canonical hash in this phase.

That decision stays reviewer-driven.

## 6. Minimum output shape

Backward-compatible additive fields are preferred.

The current `whashreonator.wwmi-anchor-report.v1` report may be extended with
new `#[serde(default)]` fields, or a new schema may be introduced if cleaner.

Minimum information needed for discovery:

- `logical_name`
- `expected_kind`
- `known_hash`
- `exact_match_found`
- `missing`
- `candidate_replacements`

For each candidate replacement:

- `hash`
- `observed_kind`
- `asset_id`
- `asset_path`
- `identity_tuple`
- `draw_call_count` if available
- `tags`
- `score`
- `reasons`

## 7. Ranking heuristics

Do not pretend there is a magical deterministic rule. The goal is to produce a
useful review-first ranking, not fake certainty.

Use only heuristics that are grounded in current code and dump structure.

Safe heuristics:

1. **Kind must match**
   - `vertex_shader` candidates only for VS anchors
   - `pixel_shader` candidates only for PS anchors
   - `texture_resource` candidates only for texture anchors
   - `compute_shader` candidates only for CS anchors

2. **Prefer records already surfaced by the current FA adapter**
   - reuse `PreparedAssetInventory`
   - reuse `identity_tuple`
   - reuse tags such as `wwmi-anchor-candidate`

3. **Prefer candidates with stronger runtime context**
   - `draw_calls=N` tags
   - non-empty `identity_tuple`
   - for textures: parent PS + slot if available from
     `fa|tex|ps:<hash>|slot:<n>`
   - for compute shaders: thread group info if available from
     `fa|cs|tg:<x>x<y>x<z>`

4. **Prefer candidates that look structurally similar to the logical anchor**
   - compute shader anchors should prefer `compute_shader` records with
     meaningful thread-group tuples
   - texture anchors should prefer `texture_resource` records with stable
     `slot`

5. **Document heuristic reasons explicitly**
   - do not hide scoring logic
   - the reviewer must see why a candidate ranked where it did

## 8. Practical guidance by anchor type

### 8.1 Compute anchors

Likely easiest and highest-value first:
- `ShapeKeyLoaderCS`
- `ShapeKeyMultiplierCS`

Current parser already captures:
- `compute_shader`
- `identity_tuple = fa|cs|tg:...`
- `wwmi-anchor-candidate` tags

This should be enough to build a useful candidate ranking slice now.

### 8.2 Texture anchors

Current parser already captures:
- `texture_resource`
- parent PS hash and slot in `identity_tuple`

This is enough for a useful review-first candidate ranking.

### 8.3 VS / PS UI anchors

These may be harder because current inventory does not preserve as much
semantic context for them as it does for textures and compute shaders.

Still, this phase should at least:
- rank same-kind candidates,
- carry draw-call count if available,
- expose enough context for a reviewer to inspect the dump.

Do not block the whole phase if VS / PS ranking remains weaker than CS / texture
ranking.

## 9. Suggested implementation strategy

Prefer the smallest additive route:

1. enrich report-side models first,
2. derive `draw_call_count` from existing `draw_calls=N` tags,
3. parse extra context from existing `identity_tuple`,
4. add a simple ranking function,
5. update tests,
6. verify against the two real shapekey dumps under `D:\mod\WWMI`.

Only if needed:
- inspect dump directory filenames to recover additional context.

Do not start with a large redesign.

## 10. Real-data verification for this phase

If the current session still has access to:
- `D:\mod\WWMI\FrameAnalysis-2026-04-22-210517`
- `D:\mod\WWMI\FrameAnalysis-2026-04-22-210709`

use them as read-only verification inputs.

Expected truth from prior verification:
- both should succeed as `shapekey_runtime`,
- neither should be treated as a valid `menu_ui` dump,
- the discovery layer should therefore be able to surface strong compute-shader
  candidate context from real data.

Do not write anything into `D:\mod\WWMI`.
Write outputs only under approved artifact roots such as `out/`.

## 11. Non-goals

Do not spend this phase on:
- GUI work,
- `.pak` parsing,
- broad whole-game compare refactors,
- automatic mod patch generation,
- automatic baseline promotion without reviewer confirmation.

## 12. Acceptance criteria

The phase is successful if:

1. current exact-match behavior still works,
2. missing anchors now produce ranked candidate replacements,
3. tests cover at least one missing-anchor discovery path,
4. real shapekey dumps can be used read-only to verify the new candidate output,
5. the final report makes it explicit that version strings do not imply hashes
   and that discovery comes from runtime dumps.

## 13. Ready handoff prompt

Use this short prompt with Codex:

```text
You are working in the Rust repository `d:\projectIT\WhashReonator`.

Before making changes, read these files first:
- `AGENTS.md`
- `.cursor/rules/codex-core-mission.mdc`
- `.cursor/rules/codex-language-reporting.mdc`
- `docs/adr-001-no-pak-parser.md`
- `docs/codex-wwmi-anchor-pivot.md`
- `docs/phase3a-wwmi-anchor-discovery.md`

Use the `backend-phase-executor` skill.

Report progress updates and the final implementation summary in Vietnamese.
Keep code symbols, file paths, commands, and literal technical values exactly as
they are.

Implement the next backend-only phase from `docs/phase3a-wwmi-anchor-discovery.md`.

Key truth to preserve:
- the project must not pretend that a new game version string like `3.3`
  directly tells us the new WWMI hashes,
- instead it must use fresh runtime dumps to discover replacement candidates for
  missing logical anchors.
```
