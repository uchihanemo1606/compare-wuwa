# WhashReonator

Rust CLI MVP for semi-automated hash mapping updates in the Wuthering Waves / WWMI workflow.

## Current scope

- End-to-end pipeline: load input -> fingerprint -> match -> validate -> export JSON
- Extensible layer boundaries for future parsers and exporters
- Prepared JSON bundle mode remains fully supported
- Additive local source scan mode for folder-to-folder comparisons
- Snapshot export mode for capturing a local game folder as a versioned asset inventory
- Snapshot report mode for converting one or more snapshot JSON files into a human-readable Markdown comparison
- Snapshot compare mode for classifying added, removed, and changed assets between versions
- WWMI knowledge extraction mode for mining historical fix patterns from git history
- Inference mode for combining snapshot diff signals with WWMI fix history
- Safe output selection for report, mapping JSON, and patch-draft JSON

## Current assumptions

1. MVP does not parse a local Wuthering Waves installation directly yet.
2. MVP expects a prepared JSON bundle with `old_assets` and `new_assets`.
3. Local scan mode currently derives assets from file paths and filenames only; it does not parse proprietary game formats.
4. Matching is heuristic and must be recalibrated once real samples are available.

## CLI usage

```powershell
cargo run -- map --input .\input.json --output .\out\mapping-report.json
```

Optional config:

```powershell
cargo run -- map --input .\input.json --output .\out\mapping-report.json --config .\config.json
```

Local folder scan with dry-run only:

```powershell
cargo run -- map-local --old-root .\snapshots\old --new-root .\snapshots\new --dry-run
```

Local folder scan with explicit outputs:

```powershell
cargo run -- map-local `
  --old-root .\snapshots\old `
  --new-root .\snapshots\new `
  --report-output .\out\mapping-report.json `
  --mapping-output .\out\mapping.json `
  --patch-draft-output .\out\patch-draft.json
```

Local folder scan with custom calibration:

```powershell
cargo run -- map-local `
  --old-root .\snapshots\old `
  --new-root .\snapshots\new `
  --report-output .\out\mapping-report.json `
  --config .\config.json
```

Create a versioned game snapshot from a local folder:

```powershell
cargo run -- snapshot `
  --source-root .\game-snapshots\2.4.0 `
  --version-id 2.4.0 `
  --output .\out\snapshot-2.4.0.json
```

For a real game install that contains `launcherDownloadConfig.json`, you can let the tool detect the version automatically:

```powershell
cargo run -- snapshot `
  --source-root "C:\Wuthering Waves\Wuthering Waves Game" `
  --version-id auto `
  --output .\out\snapshot-current.json
```

Compare two saved snapshots:

```powershell
cargo run -- compare-snapshots `
  --old-snapshot .\out\snapshot-2.4.0.json `
  --new-snapshot .\out\snapshot-2.5.0.json `
  --output .\out\snapshot-compare-2.4.0-to-2.5.0.json
```

Render one or more saved snapshots into a human-readable Markdown report:

```powershell
cargo run -- snapshot-report `
  --snapshot .\out\snapshot-2.4.0.json `
  --snapshot .\out\snapshot-2.5.0.json `
  --output .\out\snapshot-report-2.4.0-to-2.5.0.md
```

Extract heuristic fix knowledge from WWMI git history:

```powershell
cargo run -- extract-wwmi-knowledge `
  --repo https://github.com/SpectrumQT/WWMI-Package.git `
  --max-commits 200 `
  --output .\out\wwmi-knowledge.json
```

Infer probable crash causes and suggested fixes from snapshot diff + WWMI knowledge:

```powershell
cargo run -- infer-fixes `
  --compare-report .\out\snapshot-compare-2.4.0-to-2.5.0.json `
  --wwmi-knowledge .\out\wwmi-knowledge.json `
  --output .\out\inference-2.5.0.json
```

## Assumed input schema

```json
{
  "old_assets": [
    {
      "id": "old-asset-001",
      "path": "Content/Character/HeroA/Body.mesh",
      "kind": "mesh",
      "metadata": {
        "logical_name": "HeroA_Body",
        "vertex_count": 12000,
        "index_count": 18000,
        "material_slots": 3,
        "section_count": 2,
        "tags": ["hero", "body", "playable"]
      }
    }
  ],
  "new_assets": [
    {
      "id": "new-asset-001",
      "path": "Content/Character/HeroA/Body_v2.mesh",
      "kind": "mesh",
      "metadata": {
        "logical_name": "HeroA_Body",
        "vertex_count": 12000,
        "index_count": 18000,
        "material_slots": 3,
        "section_count": 2,
        "tags": ["hero", "body", "playable"]
      }
    }
  ]
}
```

## Output shape

The JSON report contains:

- `old_asset`
- `new_asset` (nullable if no candidate is found)
- `confidence`
- `status` (`matched`, `needs_review`, `rejected`)
- `reasons`

The additive machine-readable mapping output contains:

- `schema_version` (`whashreonator.mapping.v1`)
- `summary`
- `mappings`

The additive patch-draft output contains:

- `schema_version` (`whashreonator.patch-draft.v1`)
- `mode` (`draft`)
- `summary`
- `actions`

The versioned snapshot output contains:

- `schema_version` (`whashreonator.snapshot.v1`)
- `version_id`
- `created_at_unix_ms`
- `source_root`
- `asset_count`
- `assets` including normalized fingerprint fields
- `context` with optional launcher metadata and manifest coverage

The snapshot compare output contains:

- `schema_version` (`whashreonator.snapshot-compare.v1`)
- `old_snapshot`
- `new_snapshot`
- `summary`
- `added_assets`
- `removed_assets`
- `changed_assets`
- `candidate_mapping_changes`

The snapshot report output is Markdown and contains:

- a version summary table with `version`, `reuse version`, `asset count`, and resonator counts
- a resonator-by-version matrix derived from `Content/Character/<Name>/...` paths
- pairwise change tables for each adjacent version pair
- top candidate remaps rendered as a human-readable table instead of raw JSON

The WWMI knowledge output contains:

- `schema_version` (`whashreonator.wwmi-knowledge.v1`)
- `repo`
- `summary`
- `patterns`
- `keyword_stats`
- `evidence_commits`

The inference output contains:

- `schema_version` (`whashreonator.inference.v1`)
- `compare_input`
- `knowledge_input`
- `summary`
- `probable_crash_causes`
- `suggested_fixes`
- `candidate_mapping_hints`

The proposal generation flow exports mapping proposals and patch drafts derived from the inference report:

```powershell
cargo run -- generate-proposals `
  --inference-report .\out\inference-2.5.0.json `
  --mapping-output .\out\mapping-proposal-2.5.0.json `
  --patch-draft-output .\out\proposal-patch-draft-2.5.0.json `
  --summary-output .\out\summary-2.5.0.md `
  --min-confidence 0.90
```

The mapping proposal output contains:

- `schema_version` (`whashreonator.mapping-proposal.v1`)
- `compare_input`
- `knowledge_input`
- `min_confidence`
- `summary`
- `mappings`

Each mapping proposal includes:

- `old_asset_path`
- `new_asset_path`
- `confidence`
- `status` (`proposed` or `needs_review`)
- `reasons`
- `evidence`
- `related_fix_codes`

Proposal status stays `needs_review` when:

- a high-risk structural crash cause blocks the asset pair
- compare found a near-tie runner-up remap candidate
- confidence stays below the requested `--min-confidence`

The proposal patch-draft output contains:

- `schema_version` (`whashreonator.proposal-patch-draft.v1`)
- `mode` (`draft`)
- `compare_input`
- `knowledge_input`
- `min_confidence`
- `summary`
- `actions`

The human-readable summary output contains:

- version pair and WWMI knowledge context
- grouped `Fix Before Remap` and `Safe To Try Now` sections
- severity/priority ordering so the riskiest items appear first
- top inferred crash causes
- top suggested fixes
- proposed mappings
- mappings that still need review
- next-step checklist

## Local scan limitations

- `map-local` scans regular files under the provided roots and builds assets from relative paths.
- It does not patch, modify, or overwrite local game/mod files directly.
- It only writes artifacts that are explicitly requested with `--report-output`, `--mapping-output`, or `--patch-draft-output`.
- Without extra metadata or looser thresholds, local scan mode may classify many candidates as `needs_review` or `rejected`.

## Future extensions already accounted for

- Replace the ingest adapter with parsers for game-local metadata extraction
- Add separate mapping JSON readers
- Add CSV or WWMI-specific exporters

## GUI

- Build the native desktop app with `cargo build --release`, then run `.\target\release\gui.exe`.
- The desktop app now calls the core engine directly instead of shelling out to the CLI.
- `Compare` saves a managed bundle for each run under `out\reports\...` in debug/dev or `release\reports\...` in release builds.
- `Report Manager` lists saved reports, reopens old report bundles, and filters by version or resonator.
- The detailed report view is organized by old/new version, resonator, assets, buffers, and mapping candidates with status, confidence, and reasons.

## Output Policy

- Release builds must write artifacts under `release\`.
- Debug/dev workflows may continue to use `out\`.
- Artifact writes to `src\`, `tests\`, or the repository root are rejected.

## Release Guide

See [RELEASE_GUIDE.md](./RELEASE_GUIDE.md) for the full build, packaging, run, and verification flow for end users.
