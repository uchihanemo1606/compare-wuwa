# Phase 1 — Frame Analysis adapter (handoff spec for Codex)

## 1. Goal

Bridge **3DMigoto / WWMI FrameAnalysis dumps** into the existing
`PreparedAssetInventory` flow so that downstream snapshot/compare/inference/proposal
work on **runtime hashes** instead of filesystem MD5s.

After Phase 1 completes, this CLI command must work end-to-end:

```bash
cargo run --bin whashreonator -- ingest-frame-analysis \
  --dump-dir ./FrameAnalysis-2026-01-15-123456/ \
  --version-id 3.2.1 \
  --output ./out/inventory_3.2.1.json
```

The emitted JSON must validate as a `PreparedAssetInventory` and feed
`create_extractor_backed_snapshot_from_file` without changes.

## 2. Out of scope for Phase 1

- `.pak` parsing — Phase 2.
- Mod INI patching — Phase 3.
- Decoding shader source / textures from the dump — keep adapter narrow to
  vertex / index / shader hashes only.
- Tracking constant buffers, render targets, or compute shaders — XXMITools'
  reference parser leaves these commented out and so do we.

## 3. Input format spec (authoritative)

### 3.1 Files inside a `FrameAnalysis-<timestamp>/` directory

| File | Required for Phase 1 | Notes |
|---|---|---|
| `log.txt` | yes | Main per-frame log from immediate context |
| `log-0x<context>.txt` | optional | Deferred contexts; Phase 1 may ignore |
| `vb<idx>=<hash>.buf` | no (Phase 2 may use) | Raw vertex buffer bytes |
| `ib=<hash>.buf` | no (Phase 2 may use) | Raw index buffer bytes |
| `*.fmt` | no | Mesh format metadata sidecar |
| `*.txt` (shader sources) | no | Shader debug output |
| `ShaderUsage.txt` | no | Per-shader usage stats |

Phase 1 only reads `log.txt`. The other files become inputs for Phase 2 (parser
that needs raw buffer bytes for hash recomputation).

### 3.2 `log.txt` line grammar

The file starts with one header line:

```
analyse_options=<flags separated by spaces>
```

Followed by repeated drawcall blocks. A **drawcall block** is one or more
contiguous lines starting with the same drawcall counter:

```
^(?P<drawcall>\d+) <API_NAME>(<args>)
    [<slot>: [view=0x<HEX> ]resource=0x<HEX> hash=<lowercase_hex>]
    [...more bind lines...]
```

The exact regex used by the XXMITools reference parser
(`leotorrez/XXMITools/migoto/datastructures.py::FALogFile`) is:

- Drawcall start: `^(?P<drawcall>\d+) `
- Resource bind:
  `^\s+(?P<slot>[0-9D]+): (?:view=(?P<view>0x[0-9A-F]+) )?resource=(?P<address>0x[0-9A-F]+) hash=(?P<hash>[0-9a-f]+)$`

API call patterns the parser must recognize for Phase 1:

| API | Slot prefix | Phase 1 use |
|---|---|---|
| `IASetVertexBuffers(StartSlot:N, NumBuffers:M, ...)` | `vb` | Capture VB hash + slot |
| `IASetIndexBuffer(...)` | `ib` | Capture IB hash (single slot) |
| `VSSetShader(...)` | `vs` | Capture VS hash for context |
| `PSSetShader(...)` | `ps` | Capture PS hash for context |
| `DrawIndexed(IndexCount:N, ...)` | — | Captures the IndexCount of the active IB |
| `Draw(VertexCount:N, ...)` | — | Captures the VertexCount of the active VBs |
| `SOSetTargets(...)` | `so` | **Ignore** content but skip-parse cleanly |

Other API calls (`OMSetRenderTargets`, `*SetConstantBuffers`, `*SetShaderResources`)
are **not consumed**. The parser must skip them without erroring.

### 3.3 Hash format

- Always lowercase hex.
- Length usually 8 chars (32-bit FNV) but may be 16 chars in newer 3DMigoto builds.
  Treat as variable-length string `[0-9a-f]+`, do not assume 8.
- Same hash string format appears verbatim as the `hash = ...` value inside WWMI mod
  `.ini` files. This is the bridge identity Phase 3 will rely on.

### 3.4 Sample fixture

A representative synthetic log lives at:

```
tests/fixtures/sample_frame_analysis/log.txt
```

It exercises every line variant the parser must handle (multiple VB slots, IB,
VS/PS, DrawIndexed, SOSetTargets, optional `view=` prefix, repeated hashes).
A README in the same folder documents the synthetic origin and references.

When a real WWMI dump is captured, add it as a **second** fixture beside the
synthetic one (e.g. `tests/fixtures/sample_frame_analysis_wwmi_3.2.1/`) so format
coverage is documented per source.

## 4. Output schema (authoritative)

The adapter emits a JSON file matching the existing `PreparedAssetInventory`
schema in `src/domain/mod.rs`. **Do not invent new fields.** Map FA records into
the existing slots as follows:

```rust
PreparedAssetInventory {
    schema_version: "whashreonator.prepared-assets.v1".to_string(),
    context: PreparedAssetInventoryContext {
        extraction_tool: Some("3dmigoto-frame-analysis".to_string()),
        extraction_kind: Some("runtime_draw_call_hashes".to_string()),
        source_root: Some("<absolute path of dump dir>".to_string()),
        version_id: Some("<version-id passed by CLI>".to_string()),
        tags: vec!["frame-analysis".to_string(), "wwmi".to_string()],
        meaningful_content_coverage: Some(true),
        meaningful_character_coverage: Some(true), // FA hashes are character-level by definition
        note: Some("Captured from FrameAnalysis-<timestamp>/log.txt; <N> draw calls, <M> unique IB hashes, <K> unique VB hashes".to_string()),
    },
    assets: <Vec<ExtractedAssetRecord>>,
}
```

Each unique buffer hash becomes one `ExtractedAssetRecord`. **Deduplicate by hash**
across draw calls.

### 4.1 Mapping rules per asset

For each unique IB hash:

```rust
ExtractedAssetRecord {
    asset: AssetRecord {
        id: format!("ib_{}", hash),
        path: format!("runtime/ib/{}", hash),  // synthetic path scheme
        kind: Some("index_buffer".to_string()),
        metadata: AssetMetadata {
            logical_name: Some(format!("ib_{}", hash)),
            index_count: Some(<largest IndexCount observed for this IB across draw calls>),
            index_format: Some("R32_UINT".or("R16_UINT").to_string()), // from IASetIndexBuffer line if parseable
            tags: vec![format!("draw_calls={}", <count of draw calls using this IB>)],
            ..Default::default()
        },
    },
    hash_fields: AssetHashFields {
        asset_hash: Some(hash.clone()),
        shader_hash: None,
        signature: None,
    },
    source: AssetSourceContext {
        extraction_tool: Some("3dmigoto-frame-analysis".to_string()),
        source_root: Some(<dump dir>),
        source_path: Some("log.txt".to_string()),
        container_path: None,
        source_kind: Some("runtime_draw_call".to_string()),
    },
}
```

For each unique VB hash:

```rust
ExtractedAssetRecord {
    asset: AssetRecord {
        id: format!("vb_{}", hash),
        path: format!("runtime/vb/{}", hash),
        kind: Some("vertex_buffer".to_string()),
        metadata: AssetMetadata {
            logical_name: Some(format!("vb_{}", hash)),
            vertex_count: Some(<largest VertexCount observed; from DrawIndexed.IndexCount as proxy when no Draw() call exists>),
            vertex_buffer_count: Some(<number of VB slots this hash appears in>),
            tags: vec![format!("draw_calls={}", <count>)],
            ..Default::default()
        },
    },
    hash_fields: AssetHashFields {
        asset_hash: Some(hash.clone()),
        shader_hash: <Some(vs_hash) if VS appears in same draw call(s) consistently, else None>,
        signature: None,
    },
    source: AssetSourceContext { ... same shape as IB ... },
}
```

For each unique shader hash (VS / PS):

```rust
ExtractedAssetRecord {
    asset: AssetRecord {
        id: format!("vs_{}" or "ps_{}", hash),
        path: format!("runtime/{shader_kind}/{}", hash),
        kind: Some("vertex_shader".or("pixel_shader").to_string()),
        metadata: AssetMetadata {
            logical_name: Some(format!("{shader_kind}_{}", hash)),
            tags: vec![format!("draw_calls={}", <count>)],
            ..Default::default()
        },
    },
    hash_fields: AssetHashFields {
        shader_hash: Some(hash.clone()),
        asset_hash: None,
        signature: None,
    },
    source: AssetSourceContext { ... },
}
```

### 4.2 Why this mapping shape

- `asset_hash` is the field downstream `compare::score_candidate_mapping_change`
  weights heavily; we put runtime IB/VB hash there because that is the hash WWMI
  mod INI references.
- `path` is synthetic (`runtime/ib/<hash>`) but stable. Two FA dumps from different
  game versions will produce different sets of paths if hashes differ, which is
  exactly the signal compare needs.
- `vertex_count` / `index_count` give compare additional structural signal beyond
  the hash itself, so even if a re-export changes the hash, candidate mapping can
  link `(old IB with index_count 15234)` → `(new IB with index_count 15234)`.

## 5. Code structure

### 5.1 New module

`src/ingest/frame_analysis.rs` (new file). Public surface:

```rust
pub struct FrameAnalysisDump {
    pub dump_dir: PathBuf,
    pub log_path: PathBuf,
    pub options_header: String,
    pub draw_calls: Vec<FrameAnalysisDrawCall>,
}

pub struct FrameAnalysisDrawCall {
    pub drawcall: u32,
    pub vb_bindings: Vec<FrameAnalysisBinding>,  // slot + resource_address + hash
    pub ib_binding: Option<FrameAnalysisBinding>,
    pub vs_binding: Option<FrameAnalysisBinding>,
    pub ps_binding: Option<FrameAnalysisBinding>,
    pub draw: Option<FrameAnalysisDraw>,         // DrawIndexed / Draw counts
}

pub struct FrameAnalysisBinding {
    pub slot: String,                 // "0", "1", "D" for IA, etc.
    pub view_address: Option<String>, // "0xDEADBEEF" if present
    pub resource_address: String,     // "0x12345678"
    pub hash: String,                 // "4a7d9c1f"
}

pub enum FrameAnalysisDraw {
    Indexed { index_count: u32, start_index: u32, base_vertex: i32 },
    NonIndexed { vertex_count: u32, start_vertex: u32 },
}

pub fn parse_frame_analysis_log(text: &str) -> AppResult<FrameAnalysisDump>;

pub fn build_prepared_inventory(
    dump: &FrameAnalysisDump,
    version_id: &str,
) -> PreparedAssetInventory;
```

### 5.2 New CLI subcommand

Extend `src/cli/mod.rs` `Command` enum following the existing 10-subcommand pattern:

```rust
Command::IngestFrameAnalysis(IngestFrameAnalysisArgs),

#[derive(Debug, Clone, Args)]
pub struct IngestFrameAnalysisArgs {
    #[arg(long)]
    pub dump_dir: PathBuf,
    #[arg(long)]
    pub version_id: String,
    #[arg(long)]
    pub output: PathBuf,
    /// If true, also call create_extractor_backed_snapshot_from_file and persist
    /// the snapshot under report_root.
    #[arg(long)]
    pub store_snapshot: bool,
    #[arg(long)]
    pub report_root: Option<PathBuf>,
}
```

Dispatch in `src/lib.rs::run` follows the same `match` pattern used by the other
subcommands (look at `Command::Snapshot` for the closest analog).

### 5.3 New pipeline helper

`src/pipeline/mod.rs` gains:

```rust
pub fn run_ingest_frame_analysis_command(args: IngestFrameAnalysisArgs) -> AppResult<()>;
```

It must:

1. Validate `args.dump_dir` exists and contains `log.txt`.
2. Read `log.txt`, call `parse_frame_analysis_log`.
3. Call `build_prepared_inventory`.
4. Serialize to JSON with `serde_json::to_string_pretty` and write to `args.output`
   via `crate::output_policy::validate_artifact_output_path` + `std::fs::write`.
5. If `args.store_snapshot`, call existing
   `crate::snapshot::create_extractor_backed_snapshot_from_file(version_id, dump_dir, output)`
   and `report_storage::ReportStorage::save_snapshot_for_version`.
6. Print one-line summary of counts.

### 5.4 Output policy

The adapter must respect the existing safety gates:

- `validate_artifact_output_path` for `args.output` (rejects writes inside `src/`,
  `tests/`, repo root, etc.).
- `args.dump_dir` is treated as **read-only input**. Never write anywhere inside it.
- The real game root (`C:\Wuthering Waves\Wuthering Waves Game`) is **not** an input
  here at all; this command operates on a FrameAnalysis dump directory which is
  produced by 3DMigoto next to the game executable but the user copies it elsewhere
  before running the tool. Document this in the CLI help string so users do not
  accidentally point `--dump-dir` at the game folder root.

## 6. Tests (must be in `tests/*.rs`)

Per `AGENTS.md` and `.cursor/rules/codex-testing-and-artifacts.mdc`:
**no new `#[cfg(test)]` blocks inside `src/`**. All Phase 1 tests live in:

```
tests/frame_analysis_ingest.rs
```

Required test cases:

| Name | What it asserts |
|---|---|
| `parses_synthetic_fixture_log_into_drawcalls` | Calling `parse_frame_analysis_log` on `tests/fixtures/sample_frame_analysis/log.txt` returns 4 draw calls (123, 124, 125, 126) with the expected VB / IB / shader bindings. |
| `parser_handles_view_prefix_on_bind_lines` | Drawcall 126 has a `view=0xDEADBEEF` prefix; parser must extract it without dropping the binding. |
| `parser_skips_unknown_api_calls` | Insert a `OMSetRenderTargets(...)` line into a copy of the fixture; parser must not error and must not count it. |
| `dedupes_repeated_hashes_into_single_asset` | VB hash `4a7d9c1f` appears in draw calls 123 and 126; the inventory must contain exactly one `ExtractedAssetRecord` for it, with `tags` containing `draw_calls=2`. |
| `inventory_schema_round_trips_through_serde` | Serialize the produced inventory, parse it back, assert equality. |
| `cli_command_writes_inventory_under_temp_dir` | Run `run_ingest_frame_analysis_command` with `tempdir`-based output, assert file exists and parses. |
| `cli_command_rejects_writing_into_src_or_tests` | Pass `--output src/foo.json` style path, assert error from `validate_artifact_output_path`. |

Use `assert_matches!`, `serde_json::from_str`, and `tempfile::TempDir` per the
existing test conventions in `tests/snapshot_mode.rs`.

## 7. Real-root verification (mandatory per `AGENTS.md`)

After tests pass, the implementer (or the user) runs:

```bash
cargo run --bin whashreonator -- ingest-frame-analysis \
  --dump-dir <a real WWMI FrameAnalysis dump path> \
  --version-id 3.2.1 \
  --output ./out/inventory_3.2.1.json
```

And confirms:

1. The output JSON exists and is non-empty.
2. The output JSON deserializes back into `PreparedAssetInventory`.
3. **Nothing was written into `C:\Wuthering Waves\Wuthering Waves Game`.**
4. **Nothing was written into the dump directory itself.**

The final report must list those four confirmations explicitly.

## 8. Cross-references

- `src/domain/mod.rs` — `PreparedAssetInventory`, `ExtractedAssetRecord`,
  `AssetMetadata`, `AssetHashFields`, `AssetSourceContext`.
- `src/snapshot/mod.rs` — `create_extractor_backed_snapshot_from_file`.
- `src/ingest/mod.rs` — `PreparedSnapshotAssetExtractor` (consumer of the JSON we
  emit; do not need to modify).
- `src/cli/mod.rs` — existing 10 subcommands as pattern reference. Closest analog
  for the new subcommand is `Command::Snapshot`.
- `src/pipeline/mod.rs` — existing `run_*_command` helpers as pattern reference.
- `src/output_policy/mod.rs` — `validate_artifact_output_path`,
  `resolve_artifact_root`.
- `tests/snapshot_mode.rs` — closest analog for new tests in
  `tests/frame_analysis_ingest.rs`.
- `tests/fixtures/sample_frame_analysis/` — fixture + README documenting source.

## 9. External references

- 3DMigoto upstream: https://github.com/bo3b/3Dmigoto
- 3DMigoto `d3dx.ini` reference (analyse_options): see project repo
  `Dependencies/d3dx.ini` lines around `analyse_frame` / `analyse_options`.
- XXMITools reference parser:
  https://github.com/leotorrez/XXMITools/blob/main/migoto/datastructures.py
  (`FALogFile`, `FALogParserDrawcall`, `FALogParserBindResources`,
  `FALogParserIASetVertexBuffers`, `FALogParserSOSetTargets`).
- 3DMigoto hunting tutorial (confirms hash format + workflow):
  https://leotorrez.github.io/modding/guides/hunting

## 10. Acceptance checklist

Before declaring Phase 1 done:

- [ ] `cargo fmt` clean.
- [ ] `cargo test` passes including the 7 new tests above.
- [ ] No new `#[cfg(test)]` block inside `src/`.
- [ ] `PreparedAssetInventory` schema unchanged (additive use of existing fields
      only).
- [ ] CLI help text for `ingest-frame-analysis` mentions read-only dump-dir
      treatment.
- [ ] Real-root verification log included in PR description.

## 11. Anti-scope reminder

Do not extend this phase into:

- A `.pak` parser (Phase 2).
- A mod INI rewriter (Phase 3).
- GUI changes for this command.
- Changes to `compare/`, `inference/`, `proposal/`, `human_summary/`, or
  `report_storage/`. Phase 1 is purely additive at the ingest layer.

Report any urge to expand scope back to the project owner before doing it.
