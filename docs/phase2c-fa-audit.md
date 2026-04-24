# Phase 2C-FA — Audit Report

Status: **Audit complete, ready for review** (2026-04-21)
Scope: read-only audit. No code changes.

## Background

Phase 1 delivered the Frame Analysis (FA) adapter:
- Parser + builder at `src/ingest/frame_analysis.rs`.
- CLI `ingest-frame-analysis` in `src/cli/mod.rs`; pipeline dispatcher at `src/pipeline/mod.rs:390` (`run_ingest_frame_analysis_command`).
- Optional `--store-snapshot` path that calls `src/snapshot/mod.rs:514` (`create_extractor_backed_snapshot_from_file`).

Per ADR-001 (`docs/adr-001-no-pak-parser.md`), FA dumps are the canonical oracle for runtime hashes. This audit asks: can an FA-derived `PreparedAssetInventory` flow end-to-end through `snapshot → compare → continuity → inference → proposal → human_summary` and surface correctly in the GUI?

## Cross-cutting semantic differences (filesystem vs FA) — read first

These differences underlie every per-stage verdict below.

| Aspect | Filesystem mode | FA mode |
|---|---|---|
| `asset.id` | relative path like `Content/Character/Encore/Body.uasset` | `"ib_<hash>"`, `"vb_<hash>"`, `"vs_<hash>"`, `"ps_<hash>"` — **hash embedded in id** |
| `asset.path` | `Content/Character/Encore/Body.uasset` — stable across versions | `"runtime/ib/<hash>"`, `"runtime/vb/<hash>"`, etc. — **hash embedded in path** |
| `metadata.logical_name` | recoverable from filename | `"ib_<hash>"` / `"vb_<hash>"` — hash-bearing |
| `hash_fields.asset_hash` | 32-char MD5 from `LocalGameResources.json` (Kuro-provided) | 16-char FNV64/CRC32C hex from 3DMigoto |
| `hash_fields.shader_hash` | usually absent | FNV64 hex when VB has a single associated VS |
| Character attribution | `Content/Character/<Name>/...` prefix | **none** — FA data has no character hint |
| Launcher/manifest enrichment | reads `launcherDownloadConfig.json` + `LocalGameResources.json` from `source_root` (real game root) | `source_root = dump_dir`, no launcher files there; notes say "not found" but does not error |

Key implication: **path is no longer a stable identity for FA**. When a mesh changes in-game, its hash changes, so both `asset.id` and `asset.path` change. Any downstream logic keyed on `path` will report (REMOVED old + ADDED new) instead of (CHANGED hash under stable identity).

Every verdict below treats a schema change to `PreparedAssetInventory` as **out of scope**. Proposed fixes are additive only, using `#[serde(default)]` and optional fields.

---

## 1. Snapshot — `src/snapshot/mod.rs`

**Verdict: WORKS AS-IS for build path; NEEDS SMALL ADDITIVE CHANGE for scope signals.**

### Build path — WORKS AS-IS

- Entry point: `run_ingest_frame_analysis_command` passes `dump_dir` as `source_root` and the FA-produced JSON as `inventory_path` to `create_extractor_backed_snapshot_from_file` (`src/snapshot/mod.rs:514`). That delegates to `create_extractor_backed_snapshot_from_inventory` (`src/snapshot/mod.rs:490`).
- `validate_extractor_inventory_alignment` (`src/snapshot/mod.rs:794`) passes for FA because:
  - Phase 1 sets `inventory.context.version_id = Some(<user-provided --version-id>)`, and the same string is passed to `resolve_snapshot_version_id` → match succeeds.
  - `load_launcher_context(dump_dir)` returns `None` (no `launcherDownloadConfig.json` in a dump directory), so the second check is skipped.
- `enrich_snapshot_from_game_root` (`src/snapshot/mod.rs:898`) fails soft: it emits two notes (`"launcherDownloadConfig.json not found; detected_version context unavailable"` and `"LocalGameResources.json not found; asset hashes were not enriched from launcher manifest"`). Behavior is correct but the notes are noisy for FA mode.
- `annotate_extractor_inventory_alignment` (`src/snapshot/mod.rs:823`) attaches alignment notes. For FA, `launcher_version_matches_inventory = None` (no launcher) and `inventory_version_matches_snapshot = Some(true)`. Output note: `"extractor inventory alignment=aligned ..."`.

### Scope assessment — NEEDS SMALL ADDITIVE CHANGE

- `assess_snapshot_scope` (`src/snapshot/mod.rs:967`) falls back to `compute_scope_coverage` which calls `is_content_like_path` and `is_character_like_path` (`src/snapshot/mod.rs:1288, 1295`). Both require `Content/` / `Content/Character/` segments — FA paths `runtime/ib/<hash>` never match.
- Result for FA: `content_like_path_count = 0`, `character_path_count = 0`. Default heuristic sets `meaningful_content_coverage = false`, `meaningful_character_coverage = false` even though the FA snapshot IS meaningful.
- `mostly_install_or_package_level` is explicitly set to `false` when `acquisition_kind == "extractor_backed_asset_records"` (`src/snapshot/mod.rs:1001`), so that part is already FA-aware.
- Pseudo-diff:

```rust
// src/snapshot/mod.rs near fn assess_snapshot_scope (~L967)
let fa_like = acquisition_kind.as_deref() == Some("extractor_backed_asset_records")
    && snapshot
        .context
        .extractor
        .as_ref()
        .and_then(|e| e.extraction_kind.as_deref())
        == Some("runtime_draw_call_hashes");

let meaningful_content_coverage = scope.meaningful_content_coverage
    .or_else(|| fa_like.then(|| snapshot.asset_count >= MIN_MEANINGFUL_CONTENT_PATH_COUNT))
    .unwrap_or(coverage.content_like_path_count >= MIN_MEANINGFUL_CONTENT_PATH_COUNT);
let meaningful_character_coverage = scope.meaningful_character_coverage
    .or_else(|| fa_like.then(|| snapshot.asset_count >= MIN_MEANINGFUL_CHARACTER_PATH_COUNT))
    .unwrap_or(coverage.character_path_count >= MIN_MEANINGFUL_CHARACTER_PATH_COUNT);
```

This is additive: filesystem snapshots are unchanged because `fa_like = false` for them.

---

## 2. Compare — `src/compare/mod.rs` — CRITICAL

**Verdict: NEEDS SMALL ADDITIVE CHANGE. Most important finding of the audit.**

### Root issue — diff keys by `asset.path`

`SnapshotComparer::compare` (`src/compare/mod.rs:305`) builds two `BTreeMap<&str, &SnapshotAsset>` keyed on `asset.path`:

```rust
let old_by_path = old_snapshot.assets.iter()
    .map(|asset| (asset.path.as_str(), asset))
    .collect::<BTreeMap<_, _>>();
let new_by_path = new_snapshot.assets.iter()
    .map(|asset| (asset.path.as_str(), asset))
    .collect::<BTreeMap<_, _>>();
```

Then the diff loop (`src/compare/mod.rs:326-350`) treats any path in old-only as REMOVED, any in new-only as ADDED, and any shared path as a candidate for CHANGED.

For FA data, an index buffer whose hash shifts from `abc123` to `def456`:
- Old: `path = "runtime/ib/abc123"`
- New: `path = "runtime/ib/def456"`
- `old_by_path.get("runtime/ib/def456") = None` → ADDED
- `new_by_path.get("runtime/ib/abc123") = None` → REMOVED
- → **every FA hash change surfaces as a REMOVED + ADDED pair**, never as CHANGED.

### Partial mitigation that exists today

`build_candidate_mapping_changes` (`src/compare/mod.rs:1327`) does cross-product scoring over `removed × added` pairs in `score_candidate_mapping_change` (`src/compare/mod.rs:1591`). Fields that contribute confidence:

| Field | Behavior for pure FA mesh change |
|---|---|
| `kind` (e.g. `index_buffer`/`vertex_buffer`) | matches → +0.16 |
| `signature` | usually absent on FA → no contribution |
| `asset_hash` | changed → -0.04 |
| `shader_hash` | if same VS → +0.08 |
| `vertex_stride`, `vertex_buffer_count`, `index_format`, `primitive_topology` | FA currently sets at most `vertex_buffer_count` (and only in metadata, not in summary); others absent |
| `container_path` | absent for FA |
| `logical_name` | FA sets `"ib_<hash>"`; hashes differ → mismatch penalty likely |

Best case (same character, shader hash preserved): ~0.16 + 0.08 − 0.04 = **~0.20**. Reportable confidence threshold is `0.65` (`src/compare/mod.rs:1331`). **FA mesh changes will not clear the threshold via candidate mapping alone.**

### Additional issues in compare

- `logical_name` similarity helper at `src/compare/mod.rs:822-824` compares `metadata.logical_name`. FA sets it to `"ib_<hash>"`, so it always mismatches when the hash changes. Penalizes candidates that should be paired.
- `asset_hash_mismatch` is only a -0.04 penalty, which is correct for filesystem mode (MD5 change often means true asset replacement). For FA that penalty is semantically wrong: a changed runtime hash is the **normal** signal we came to observe.

### Pseudo-diff — introduce optional stable identity

```rust
// src/compare/mod.rs:305 — key diff by stable identity when both sides supply one
fn diff_key(asset: &SnapshotAsset) -> &str {
    asset
        .fingerprint
        .identity_tuple           // NEW optional stable tuple, serde(default)
        .as_deref()
        .unwrap_or(asset.path.as_str())   // fallback = current behavior
}

let old_by_key = old_snapshot.assets.iter()
    .map(|a| (diff_key(a), a))
    .collect::<BTreeMap<_, _>>();
let new_by_key = new_snapshot.assets.iter()
    .map(|a| (diff_key(a), a))
    .collect::<BTreeMap<_, _>>();
```

Plus a Phase 1 change (accepted by this audit as additive): FA inventory populates `identity_tuple` with something like `format!("{kind}|shader:{shader_hash}|idx:{index_count}|vstride:{stride}")`. When two FA snapshots share the same draw-call semantics, the tuple stays stable while `asset_hash` shifts → compare emits CHANGED correctly.

Filesystem snapshots leave `identity_tuple = None` → behavior unchanged.

### Quality counters

`SnapshotCompareScopeInfo` counters (`src/compare/mod.rs:47-80`) like `assets_with_asset_hash`, `assets_with_any_hash`, `manifest_matched_assets` all work from `hash_fields` directly and are format-agnostic. **WORKS AS-IS.** One nit: `manifest_*` fields will always be zero for FA snapshots, which is correct but may look alarming in reports without context. Covered by scope notes.

---

## 3. Continuity — `src/report/mod.rs` and `src/report_storage/mod.rs`

**Verdict: NEEDS SMALL ADDITIVE CHANGE. Same root cause as compare.**

Note: there is **no** `src/continuity/` directory. Continuity logic lives in `src/report/mod.rs` via `VersionContinuityIndex`, `VersionContinuityThread`, etc.; persistence helpers live in `src/report_storage/mod.rs`.

### Finding

- `VersionedItem` has a `key: String` field (`src/report/mod.rs:273`). The implementation `impl From<&SnapshotAssetSummary> for VersionedItem` sets `key = value.path.clone()` (`src/report/mod.rs:1556-1559`).
- Thread identity is built as `format!("{}:{}", input.to_version_id, item.key)` (`src/report/mod.rs:843`).
- So for FA, a single draw call whose hash shifts every version produces a **new thread per version**, never a continuing lineage.

### Pseudo-diff — track stable identity in continuity too

```rust
// src/report/mod.rs near impl From<&SnapshotAssetSummary> for VersionedItem (~L1556)
impl From<&SnapshotAssetSummary> for VersionedItem {
    fn from(value: &SnapshotAssetSummary) -> Self {
        Self {
            key: value
                .identity_tuple          // uses the same new field as compare
                .clone()
                .unwrap_or_else(|| value.path.clone()),
            // ... other fields unchanged
        }
    }
}
```

Once compare and continuity both read `identity_tuple` with path fallback, FA continuity threads stay stable across hash shifts and filesystem threads are unchanged.

---

## 4. Inference — `src/inference/mod.rs`

**Verdict: MOSTLY WORKS AS-IS.**

- `InferredMappingHint` (`src/inference/mod.rs:198`) stores `old_asset_path: String` and `new_asset_path: String`. Accepts any string content; FA paths `runtime/ib/<hash>` fit.
- Inference pulls from `CandidateMappingChange` produced by compare (`src/compare/mod.rs:1327`). So if compare's candidate mapping fails to pair FA hash changes (see §2), inference produces nothing for them — not because inference is broken, but because it has no signal to work from.
- No 32-char MD5 assumption found (`grep` confirms: the only MD5 reference in `src/` is a test fixture string in `src/snapshot/mod.rs:1499`). Hash length / format is not parsed anywhere in inference.

**Dependent on §2.** Once compare pairs FA transitions correctly, inference fires automatically.

Minor additive improvement described in §5.

---

## 5. Proposal — `src/proposal/mod.rs`

**Verdict: WORKS AS-IS structurally; SMALL ADDITIVE CHANGE recommended to make output directly consumable by a future mod-INI patcher.**

- `MappingProposalEntry` (`src/proposal/mod.rs:49`) carries `old_asset_path` / `new_asset_path` strings. For FA these are `runtime/ib/<hash>` strings — information survives.
- `ProposalPatchDraftAction.target = format!("{} -> {}", mapping.old_asset_path, mapping.new_asset_path)` (`src/proposal/mod.rs:318`). For FA, the target string becomes `"runtime/ib/abc123 -> runtime/ib/def456"`. A patcher can parse hashes out of the path but this is brittle.

### Pseudo-diff — expose raw hashes as first-class fields

```rust
// src/proposal/mod.rs:49
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MappingProposalEntry {
    pub old_asset_path: String,
    pub new_asset_path: String,
    #[serde(default)]
    pub old_runtime_hash: Option<String>,  // NEW
    #[serde(default)]
    pub new_runtime_hash: Option<String>,  // NEW
    pub confidence: f32,
    // ...existing fields unchanged
}
```

Populate from the matched `SnapshotAssetSummary.asset_hash` on both sides. Downstream consumers (human_summary, future mod-INI patcher) become trivial. Additive, backward-compatible.

Same optional pair could be added to `InferredMappingHint` in `src/inference/mod.rs:198` to keep the flow consistent.

---

## 6. Human summary — `src/human_summary/mod.rs`

**Verdict: WORKS AS-IS but produces low-signal output for FA.**

- `render_mapping` (`src/human_summary/mod.rs:602`) formats mappings as ``- `{old}` -> `{new}` [{status} {confidence}]``. For FA: ``- `runtime/ib/abc123` -> `runtime/ib/def456` [needs_review 0.700]``. Readable, not misleading, but low signal: a modder sees hashes without character context.
- Resonator/character grouping is inherited from `infer_resonator_name` in `src/report/mod.rs:1811`, which returns `Some(name)` only when path contains `Content/Character/<Name>/`. For FA paths, always returns `None` → resonator defaults to `"Unknown"` (`src/report/mod.rs:1802`).
- Consequence: every FA mapping lands under a single "Unknown" character bucket in the rendered summary. No character-level breakdown.

Not a correctness bug. Remediation (§phase-2c-fa-5 below) needs a character hint **upstream** — FA data alone cannot recover this.

---

## 7. GUI — `src/gui_app/mod.rs` + `src/bin/gui.rs`

**Verdict: Version Library WORKS AS-IS. Compare table WORKS AS-IS structurally but groups FA rows under "Unknown" — same issue as §6.**

### Version Library

- `ReportStorage::list_versions` (`src/report_storage/mod.rs:482`) enumerates version directories by parsed folder name. Snapshots stored via `ingest-frame-analysis --store-snapshot` land under the same version layout and appear in the GUI's Version Library with no special handling needed. **WORKS AS-IS.**

### Compare table

- `build_compare_table_rows` (`src/gui_app/mod.rs:828`) iterates `ResonatorDiffEntry` → `items`. For each item, renders:
  - `resonator`: inherited from `infer_resonator_name` — "Unknown" for FA (see §6).
  - `path`: `item.new.path OR item.old.path OR item.old.label`. For FA this is `runtime/ib/<hash>`.
  - `asset_hash`: `format_hash_transition(old_asset_hash, new_asset_hash)` — works for FA.
  - `shader_hash`: same — works for FA.
  - `status`: `"Added"` / `"Removed"` / `"Changed"` — will read `"Added"`/`"Removed"` a lot due to §2.
- Once §2 is fixed (stable identity), most FA rows will render as `"Changed"` with `asset_hash: abc123 → def456`, which is exactly what a reviewer wants to see.

---

## Existing FA test coverage vs. what Phase 2C-FA would need

### Current (`tests/frame_analysis_ingest.rs`, 7 tests)

Coverage is strong for Phase 1 scope:
- `parses_synthetic_fixture_log_into_drawcalls`
- `parser_handles_view_prefix_on_bind_lines`
- `parser_skips_unknown_api_calls`
- `dedupes_repeated_hashes_into_single_asset`
- `inventory_schema_round_trips_through_serde`
- `cli_command_writes_inventory_under_temp_dir`
- `cli_command_rejects_writing_into_src_or_tests`

All parser/ingest-layer. None exercise snapshot → compare → inference → proposal on FA data.

### Missing for Phase 2C-FA acceptance

- FA snapshot through compare: 2 FA fixtures (same draw calls, one hash changes) → compare emits exactly 1 CHANGED entry, 0 added, 0 removed.
- FA snapshot scope: ≥5 draw calls → `meaningful_*_coverage = true`.
- FA continuity: 3 versions, same draw call, hash shifts each version → single continuity thread spanning all 3.
- FA proposal runtime hash fields: `old_runtime_hash` / `new_runtime_hash` populated when both sides carry `asset_hash`.
- FA + filesystem mixed compare (old = filesystem, new = FA): documented behavior (likely all REMOVED + all ADDED; low-signal flag set).

All of these go under `tests/*.rs` per AGENTS.md. No new inline `#[cfg(test)]` in `src/`.

---

## Edge cases surfaced during the audit

| Edge case | Current behavior | Proposed handling |
|---|---|---|
| Empty FA dump (0 draw calls) | `build_prepared_inventory` returns an inventory with `assets = []`; snapshot is built with `asset_count = 0`. Compare against another empty snapshot emits no diff. Against a non-empty snapshot, flags all as REMOVED or ADDED. | Acceptable. Surface via `low_signal_compare` path that already exists. |
| FA dump with only IB bindings (no VB) | `build_prepared_inventory` still produces IB records; VB aggregator just has no entries. Compare works per-kind. | WORKS AS-IS. |
| Draw call with no shader binding | `shader_hash = None` on the VB record. Compare's shader_hash scoring simply skips; no penalty. | WORKS AS-IS. |
| Multiple draw calls sharing a buffer hash | `aggregate_ib_assets` / `aggregate_vb_assets` dedupe in Phase 1; `draw_calls` tag records the multiplicity. | WORKS AS-IS (and already regression-tested by `dedupes_repeated_hashes_into_single_asset`). |
| FA snapshot compared against filesystem snapshot (mixed modes) | Every path disjoint → all REMOVED on one side, all ADDED on the other. `candidate_mapping_changes` may pair some via kind + shader_hash but most will fall below reportable confidence. | Acceptable as long as `low_signal_compare` is flagged. Consider explicit note when `acquisition_kind` differs between sides. |
| FA snapshot with `context.version_id` missing | `validate_extractor_inventory_alignment` skips the match check, `annotate_extractor_inventory_alignment` tags alignment as `"declared_but_unverified"` or `"undeclared"`. | WORKS AS-IS. |
| FA snapshot with `version_id` reused across different dumps | Phase 1 trusts the caller. If user re-runs with same `--version-id`, the snapshot just overwrites at the `ReportStorage` level (normal behavior). | WORKS AS-IS. |

---

## Proposed follow-up phases

Every slice is **additive and backward-compatible**. No schema-breaking changes to `PreparedAssetInventory`, `GameSnapshot`, or any report type. All new tests live in `tests/*.rs`.

### phase-2c-fa-1 — Stable identity for FA snapshots (~1.5 days) — **unblocks everything else**

- File scope: `src/domain/mod.rs` (add optional field), `src/snapshot/mod.rs` (populate it from inventory), `src/ingest/frame_analysis.rs` (compute tuple), `src/compare/mod.rs` (read it in diff key + scoring), `src/report/mod.rs` (read it in continuity), `tests/frame_analysis_identity.rs` (new).
- Change: add `identity_tuple: Option<String>` with `#[serde(default)]` to `AssetFingerprint` or `SnapshotAssetSummary` (pick whichever is already on the compare/continuity hot path). For FA, populate with `format!("{}|shader:{}|idx:{}|vstride:{}|vbcount:{}", kind, shader_hash_or_none, index_count_or_none, stride_or_none, vb_count_or_none)`. For filesystem snapshots, leave `None`.
- Compare + continuity: prefer `identity_tuple` over `path` for diff key and for `VersionedItem.key`. Fall back to `path` when `None`.
- Test plan: 2 FA fixtures on same draw call (same shader_hash + same index_count, different asset_hash). Run compare. Assert exactly 1 CHANGED entry with `asset_hash` in `changed_fields`. All tests in `tests/*.rs`.
- Acceptance criterion: compare report against two synthetic FA snapshots of the same draw call with only `asset_hash` differing produces 1 CHANGED asset and 0 added / 0 removed.

### phase-2c-fa-2 — FA-aware scope signals (~0.5 day)

- File scope: `src/snapshot/mod.rs`, `tests/frame_analysis_snapshot_scope.rs` (new).
- Change: in `assess_snapshot_scope`, when `acquisition_kind == "extractor_backed_asset_records"` AND the inventory's `extraction_kind == "runtime_draw_call_hashes"`, derive `meaningful_content_coverage` / `meaningful_character_coverage` from `snapshot.asset_count` against the existing thresholds (`MIN_MEANINGFUL_CONTENT_PATH_COUNT`, `MIN_MEANINGFUL_CHARACTER_PATH_COUNT`) instead of path-prefix heuristics. Attach a scope note that explains the FA-branch was taken.
- Test plan: fixture FA snapshot with ≥10 synthetic draw calls → `meaningful_content_coverage = true`. Fixture filesystem snapshot with only 3 `Content/` paths still reports `meaningful_content_coverage = false` (regression).
- Acceptance criterion: FA snapshot with ≥10 draw calls is not marked as low-signal solely on path-prefix grounds.

### phase-2c-fa-3 — Runtime hash fields in inference + proposal (~0.5–1 day)

- File scope: `src/inference/mod.rs` (add fields to `InferredMappingHint`), `src/proposal/mod.rs` (add fields to `MappingProposalEntry` + populate), `src/human_summary/mod.rs` (optionally surface them), `tests/frame_analysis_proposal.rs` (new).
- Change: add `old_runtime_hash: Option<String>` + `new_runtime_hash: Option<String>` with `#[serde(default)]` to both types. Fill from `SnapshotAssetSummary.asset_hash` on matched candidates.
- Test plan: given an FA compare with one paired CHANGED asset, proposal output contains populated `old_runtime_hash` / `new_runtime_hash` that equal the source FA hashes.
- Acceptance criterion: proposal JSON for an FA-driven mapping carries the raw runtime hashes in structured fields (not only inside path strings).

### phase-2c-fa-4 — Continuity works across FA hash shifts (~0.5 day; rides on 2c-fa-1)

- File scope: `src/report/mod.rs`, `tests/version_continuity_fa.rs` (new).
- Change: confirm `VersionedItem::from(&SnapshotAssetSummary)` and downstream thread IDs use the new `identity_tuple` when present. Mostly a consequence of 2c-fa-1; this phase adds explicit tests.
- Test plan: three FA snapshots (v1, v2, v3) where the same draw call has a different hash in every version. Build continuity from `compare(v1, v2)` and `compare(v2, v3)`. Assert a single thread spanning all three versions.
- Acceptance criterion: continuity index for three FA snapshots with hash drift contains exactly one thread of length 3, not three length-1 threads.

### phase-2c-fa-5 — Optional character hint for FA (~0.5 day)

- File scope: `src/cli/mod.rs` (add `--character-hint` to `IngestFrameAnalysisArgs`), `src/pipeline/mod.rs` (pass it through), `src/ingest/frame_analysis.rs` (record it), `src/domain/mod.rs` (extend `PreparedAssetInventoryContext` additively with `character_hint: Option<String>`), `src/report/mod.rs::infer_resonator_name` (prefer explicit hint when present), `tests/frame_analysis_character_hint.rs` (new).
- Change: fully additive. Filesystem flow untouched.
- Test plan: ingest fixture log with `--character-hint Encore`, store snapshot, compare against a second FA snapshot, verify GUI-level `ResonatorDiffEntry` groups under `"Encore"` instead of `"Unknown"`.
- Acceptance criterion: with `--character-hint Encore`, the rendered diff report lists changes under `Encore`, not `Unknown`.

### phase-2c-fa-6 — Cleaner "source_root not a game folder" notes (optional, ~0.25 day)

- File scope: `src/snapshot/mod.rs`.
- Change: in `enrich_snapshot_from_game_root`, when `acquisition_kind` indicates FA (via `snapshot.context.extractor.extraction_kind == "runtime_draw_call_hashes"`), suppress the two "not found" notes or replace them with `"FA snapshot: launcher/manifest enrichment intentionally skipped"`. Keep the filesystem wording for filesystem snapshots.
- Acceptance criterion: FA snapshot context notes no longer mention missing launcher/manifest artifacts as if they were a scan failure.

---

## Not in scope of Phase 2C-FA (defer)

- Mod INI patcher (the "last mile" that consumes proposal output). Tracked separately after Phase 2C-FA lands.
- Pak extraction / `.uasset` decode / offline runtime-hash regeneration. Explicitly **rejected** in ADR-001.
- FA + filesystem cross-mode merging (treating FA as enrichment of a filesystem snapshot). Could be a future `phase-2c-fa-7` if ever needed, but there is no current requirement.

---

## Summary

The end-to-end FA pipeline is structurally in place (Phase 1 already builds the snapshot correctly). Two surgical additive changes unblock everything downstream:

1. **Stable identity for FA assets** (phase-2c-fa-1) — compare and continuity currently key on a path that contains the hash; fix with an optional `identity_tuple` alongside `path`.
2. **Scope heuristics aware of FA mode** (phase-2c-fa-2) — path-prefix heuristics don't fire on `runtime/...` paths; add an FA branch with threshold on `asset_count`.

The remaining slices (runtime hash fields in proposal, character hint, note cleanup) are polish.

No schema breaks. No reintroduction of pak parsing. No new runtime dependencies.
