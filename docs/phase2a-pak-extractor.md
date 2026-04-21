# Phase 2A — `.pak` extractor (handoff spec for Codex)

## 1. Goal

Add a CLI subcommand that **extracts WuWa `.pak` archives** (UE4.26 format, AES-encrypted) to a local directory. Output is the raw `.uasset` / `.uexp` / `.ubulk` file tree that Phase 2B will later parse into mesh data.

After Phase 2A completes, this CLI command must work end-to-end:

```bash
cargo run --bin whashreonator -- extract-pak \
  --pak C:\path\to\pakchunk0-WindowsNoEditor.pak \
  --aes-key 0xE0D4C0AA387A268B29C397E3C0CAD934522EFC96BE5526D6288EA26351CDACC9 \
  --output-dir ./out/extracted_3.2.1/ \
  --path-filter "Content/Character/**"
```

Output:
- `./out/extracted_3.2.1/<asset path tree mirroring inside the pak>`
- One-line summary printed: `extracted 1342 files (892 MB) in 14.3s; filtered out 4521 entries`

## 2. Out of scope for Phase 2A

Phase 2A is **pak extraction only**. Do NOT implement any of the following — they are explicitly Phase 2B or Phase 2C:

- Parsing `.uasset` content (mesh decoding, texture decoding) — Phase 2B
- Computing 3DMigoto-equivalent runtime hashes — Phase 2C
- Generating `PreparedAssetInventory` from extracted assets — Phase 2C
- Mod INI patching — Phase 3
- AES key auto-fetch from community endpoints — separate hardening phase later
- GUI surface for the new command — separate UI phase
- Re-pack `.pak` (write back) — never needed; tool is read-only on game data

## 3. Input format spec (authoritative)

### 3.1 Pak file format

WuWa uses **UE4.26 pak format** with **AES encryption** on both index and data sections. The `repak` Rust crate (https://github.com/trumank/repak) supports this format directly.

Each entry inside the pak has:
- `path`: forward-slash-separated path inside the virtual filesystem (e.g. `Content/Character/Encore/Body.uasset`)
- `compressed_size`, `uncompressed_size`
- `compression_method`: usually `Zlib`, `Oodle`, or `None`
- `hash`: pak-internal SHA1 (NOT the runtime hash WWMI mods use; ignore for Phase 2A)
- raw byte payload

### 3.2 AES key format

WuWa AES keys are **256-bit (32 bytes)**, expressed as **64 lowercase or uppercase hex characters**, optionally prefixed with `0x`. Examples:

```
0xE0D4C0AA387A268B29C397E3C0CAD934522EFC96BE5526D6288EA26351CDACC9
E0D4C0AA387A268B29C397E3C0CAD934522EFC96BE5526D6288EA26351CDACC9
```

Both forms must be accepted. Reject any other length or non-hex content.

Reference key archive (read-only public repo, never commit a key into our repo):
- `https://github.com/ClostroOffi/wuwa-aes-archive`

### 3.3 Path filter

Optional glob filter on entry paths inside the pak. Use the `glob` crate's `Pattern` type if dependency is available (see section 3.5), otherwise a simple `starts_with` + `**` substring match is acceptable for v1.

For Phase 2A, the only filter use case the user cares about is `Content/Character/**` (only character meshes, skip engine binaries / shaders / audio). Anything more sophisticated is Phase 2B+ concern.

### 3.4 Sample fixture

We cannot ship a real WuWa pak (it is encrypted game content). Instead, build a **synthetic test pak** programmatically using `repak`'s write capability:

`tests/fixtures/sample_pak/build_fixture.rs` (a `[dev-dependencies]` helper, OR a `build.rs`-style generator, OR a one-shot `cargo test` that generates and reuses):

- Creates an unencrypted UE4.26 pak with 4-6 fake entries:
  - `Content/Character/Encore/Body.uasset` (10 KB random bytes)
  - `Content/Character/Encore/Body.uexp` (50 KB random bytes)
  - `Content/Character/Carlotta/Hair.uasset` (8 KB)
  - `Engine/Binaries/Win64/dummy.dll` (4 KB)
  - `Audio/Music/dummy.wem` (12 KB)
- Pak is unencrypted (no AES key required for read in tests).
- Fixture is regenerated on first test run if missing.

The synthetic pak validates extraction logic without exposing real game data.

A separate fixture for AES-encrypted pak is **optional**. If `repak` test fixtures expose one, reuse; else add a `cargo test --ignored` test that the user runs once with a real key+pak to validate AES path.

### 3.5 New dependency

Add to `Cargo.toml`:

```toml
[dependencies]
repak = "0.x"  # latest stable that supports UE4.26 + AES read
```

If `glob` crate is not already in deps, prefer NOT to add it; use a simple substring match for path filter v1.

This is an **explicit dependency addition**, the only one allowed for Phase 2A. STOP and report blocker if you find you need any other new dep (no `tokio`, no `clap` extension, no `serde_json` rewrite — work with what is already in the project).

## 4. Output spec

### 4.1 Directory layout

After extraction, `--output-dir` contains the entry tree mirrored from inside the pak:

```
out/extracted_3.2.1/
├── Content/
│   └── Character/
│       ├── Encore/
│       │   ├── Body.uasset
│       │   └── Body.uexp
│       └── Carlotta/
│           └── Hair.uasset
└── extraction_manifest.v1.json
```

### 4.2 `extraction_manifest.v1.json`

A small JSON sidecar describing the extraction run, written into the output dir root. Schema (define a new struct in `src/ingest/pak_extractor.rs`, do not pollute `src/domain/mod.rs`):

```json
{
  "schema_version": "whashreonator.pak-extraction-manifest.v1",
  "pak_source": {
    "path": "C:/path/to/pakchunk0-WindowsNoEditor.pak",
    "size_bytes": 5234567890,
    "mount_point": "../../../"
  },
  "aes_key_used": true,
  "extracted_at_unix_ms": 1776395937932,
  "filter_pattern": "Content/Character/**",
  "summary": {
    "total_entries_in_pak": 5863,
    "extracted_entries": 1342,
    "filtered_out_entries": 4521,
    "extracted_bytes": 935123456,
    "duration_ms": 14312
  },
  "entries": [
    {
      "pak_path": "Content/Character/Encore/Body.uasset",
      "output_relative_path": "Content/Character/Encore/Body.uasset",
      "size_bytes": 10240,
      "compression": "Zlib"
    }
    // ... more entries (do NOT include all 1342 in the JSON for performance;
    // cap at 1000 entries with a "truncated_entries: 342" sibling field)
  ]
}
```

This manifest is **diagnostic only** — Phase 2B reads files from disk, not from the manifest. The manifest exists so a reviewer can sanity-check what the extractor did without inspecting the file tree.

### 4.3 What the adapter MUST NOT do

- Do NOT print AES key to console or write it into any file (manifest stores `aes_key_used: bool`, never the key itself).
- Do NOT extract files outside `--output-dir` even if pak entries claim path traversal (`../`). Validate every output path stays under `output-dir`.
- Do NOT write into the pak source location (game install). Read-only.
- Do NOT skip `validate_artifact_output_path` on `--output-dir`.

## 5. Code structure

### 5.1 New module: `src/ingest/pak_extractor.rs`

```rust
use std::path::{Path, PathBuf};
use crate::error::AppResult;

pub struct PakExtractionRequest {
    pub pak_path: PathBuf,
    pub aes_key: Option<AesKey>,        // None → assume unencrypted pak
    pub output_dir: PathBuf,
    pub path_filter: Option<String>,     // glob-like; for v1 use substring match
}

pub struct AesKey {
    bytes: [u8; 32],
}

impl AesKey {
    pub fn from_hex(hex: &str) -> AppResult<Self>;  // strips optional 0x, lowercases, validates
    // Do NOT implement Display or Debug to avoid accidental log leaks.
    // Provide an explicit `as_bytes()` that internal callers can use sparingly.
    pub(crate) fn as_bytes(&self) -> &[u8; 32];
}

pub struct PakExtractionResult {
    pub manifest: PakExtractionManifest,
    pub manifest_path: PathBuf,
}

pub fn extract_pak(request: PakExtractionRequest) -> AppResult<PakExtractionResult>;

// Public for serde + tests
pub struct PakExtractionManifest { /* matches 4.2 schema */ }
pub struct PakExtractionEntry    { /* matches 4.2 schema */ }
pub struct PakExtractionSummary  { /* matches 4.2 schema */ }
pub struct PakSourceInfo         { /* matches 4.2 schema */ }
```

### 5.2 New CLI subcommand: `src/cli/mod.rs`

Follow the existing 11-subcommand pattern. Closest analog: `Command::Snapshot` for `--output` + `--report-root` shape.

```rust
#[derive(Debug, Clone, Args)]
pub struct ExtractPakArgs {
    /// Path to the .pak file (read-only). Must NOT point inside the live game folder
    /// if outputs would land elsewhere — but reading from the live game folder is OK.
    #[arg(long)]
    pub pak: PathBuf,

    /// 64-char hex AES-256 key (with or without 0x prefix). If omitted, the pak is
    /// assumed to be unencrypted.
    #[arg(long, conflicts_with = "aes_key_file")]
    pub aes_key: Option<String>,

    /// Path to a text file whose first line is a 64-char hex AES-256 key.
    /// Useful so the key never appears in shell history.
    #[arg(long, conflicts_with = "aes_key")]
    pub aes_key_file: Option<PathBuf>,

    /// Output directory for extracted files. Will be created if missing.
    /// Subject to validate_artifact_output_path.
    #[arg(long)]
    pub output_dir: PathBuf,

    /// Optional path filter (substring match for v1). Example: "Content/Character/".
    /// Entries whose pak path does not contain this substring are skipped.
    #[arg(long)]
    pub path_filter: Option<String>,
}
```

Add the variant to `Command` enum:

```rust
ExtractPak(ExtractPakArgs),
```

### 5.3 Dispatcher: `src/lib.rs`

Add one arm to the existing `match` in `pub fn run(cli: Cli)`:

```rust
Command::ExtractPak(args) => {
    pipeline::run_extract_pak_command(args)?;
}
```

### 5.4 Pipeline helper: `src/pipeline/mod.rs`

```rust
pub fn run_extract_pak_command(args: ExtractPakArgs) -> AppResult<()> {
    validate_artifact_output_path(args.output_dir.as_path())?;

    // 1. Validate pak source exists, is a file, is readable.
    // 2. Resolve AES key from either --aes-key inline or --aes-key-file.
    //    Reject if BOTH or NEITHER provided AND the pak header indicates encryption.
    // 3. Build PakExtractionRequest, call extract_pak.
    // 4. On success, print one-line summary to stdout.
    // 5. NEVER print the AES key.
}
```

### 5.5 Output policy integration

`validate_artifact_output_path(output_dir)` must be called **before** any extraction starts. Reuse the existing function from `src/output_policy/mod.rs`. Do not add new validation paths.

For path traversal protection inside the pak (entry path with `..`), implement a defensive check: after resolving each output path, assert it is still under `output-dir.canonicalize()`. If not, error out with a clear message. This is a security boundary, not optional.

## 6. Tests (must be in `tests/*.rs`)

Per `AGENTS.md` and `.cursor/rules/codex-testing-and-artifacts.mdc`: **no new `#[cfg(test)]` blocks inside `src/`**. All Phase 2A tests live in:

```
tests/pak_extractor.rs
```

Required test cases:

| Name | What it asserts |
|---|---|
| `extracts_synthetic_unencrypted_pak_to_temp_dir` | Build a 4-entry unencrypted pak via repak in a tempdir, run `extract_pak`, assert all 4 files exist on disk with correct content and the manifest lists all 4. |
| `applies_path_filter_substring_match` | Same fixture, pass `path_filter = "Content/Character/"`, assert only character files extracted, manifest summary shows correct `filtered_out_entries`. |
| `aes_key_from_hex_accepts_with_and_without_prefix` | `AesKey::from_hex("0xAABB...CC")` and `AesKey::from_hex("AABB...CC")` both succeed and produce the same bytes. |
| `aes_key_from_hex_rejects_invalid_length_or_chars` | `AesKey::from_hex("AABB")` errors. `AesKey::from_hex("XX...XX")` errors. |
| `aes_key_does_not_appear_in_debug_or_display` | Construct an `AesKey`, format it via `{:?}` and `{}` (if Display is implemented for some wrapper), assert the hex never appears in the resulting string. (If you correctly omitted Display impl, just assert `format!("{:?}", key)` does not contain the hex digits.) |
| `manifest_serializes_to_expected_schema_version` | After extraction, parse the manifest JSON, assert `schema_version == "whashreonator.pak-extraction-manifest.v1"` and `aes_key_used == false` for the unencrypted fixture. |
| `cli_command_writes_manifest_under_temp_dir` | Run `run_extract_pak_command` end-to-end with a tempdir output, assert manifest.v1.json exists, parses, and lists ≥1 entry. |
| `cli_command_rejects_writing_into_src_or_tests` | Pass `--output-dir src/extracted/`, assert error from `validate_artifact_output_path`. |
| `pak_entry_with_path_traversal_is_rejected` | If feasible without too much fixture wiring, build a pak with an entry whose internal path contains `..` and assert the extractor refuses to write outside `output-dir`. If hard to build, write a unit test on the path-resolution helper directly. |

## 7. Real-root verification (mandatory per `AGENTS.md`)

Per `.cursor/rules/codex-real-root-verification.mdc`. Real WuWa game root: `C:\Wuthering Waves\Wuthering Waves Game`.

For Phase 2A specifically:

> The new `extract-pak` command MAY READ from `C:\Wuthering Waves\Wuthering Waves Game\Client\Content\Paks\*.pak` (the user explicitly provides the `.pak` path). It MUST NEVER WRITE there. All extracted files go to the user-provided `--output-dir`, which is validated by `validate_artifact_output_path`.

Confirm by:

1. Code review: search for any `fs::write`, `fs::create_dir`, or `repak::pack` invocation that does not target a path under `output-dir`.
2. Smoke test (synthetic fixture, no real game pak required):
   ```
   cargo run --bin whashreonator -- extract-pak \
     --pak target/test_fixtures/sample.pak \
     --output-dir target/tmp/extract-smoke/
   ```
3. Final report explicitly states no command path can write into `C:\Wuthering Waves\Wuthering Waves Game`.

## 8. Working procedure

### Step 1 — Restate the goal (English, internally)

3-line restatement of Phase 2A. Do not start coding until you can state it cleanly.

### Step 2 — Inspect git worktree

`git status`. Confirm clean. Confirm branch is `developer`. If not clean / not on `developer`, STOP and report.

### Step 3 — Add the `repak` dependency

Edit `Cargo.toml`. Add `repak = "0.x"` (latest stable). Run `cargo build --lib`. If repak pulls a transitive dep that conflicts with existing project deps, STOP and report blocker — do not work around by pinning unrelated deps.

### Step 4 — Implement `AesKey` and helpers (no I/O yet)

`src/ingest/pak_extractor.rs` skeleton with `AesKey::from_hex`, `as_bytes`, and the manifest structs (with `serde::Serialize` derives). Keep the API minimal.

### Step 5 — Verification gate A

`cargo build --lib`. Clean compile required.

### Step 6 — Implement `extract_pak` core

Read the pak using `repak`. Iterate entries. Apply filter. Write each entry to the output directory. Build the manifest as you go. Skip path-traversal entries with an error message but do not abort the whole extraction (log + continue).

### Step 7 — Verification gate B

`cargo build --lib`. Clean compile.

### Step 8 — Add tests

Create `tests/pak_extractor.rs` with the cases from section 6. Run `cargo test --test pak_extractor`. All tests must pass before proceeding.

### Step 9 — Wire CLI subcommand

`src/cli/mod.rs` + `src/lib.rs` + `src/pipeline/mod.rs::run_extract_pak_command`.

### Step 10 — Verification gate C

`cargo build --bin whashreonator`. Clean compile. Run `cargo run --bin whashreonator -- extract-pak --help` and verify the help text appears, the AES key flags conflict properly (`--aes-key` and `--aes-key-file` mutually exclusive).

### Step 11 — Smoke test

Run the smoke test from section 7. Confirm output dir exists, files extracted, manifest valid.

### Step 12 — Run full test suite

`cargo fmt && cargo test`. ALL existing tests still pass + the new tests.

### Step 13 — Real-root verification

Per section 7. Code review + smoke test + explicit confirmation.

### Step 14 — Final Vietnamese report

Per template in section 11.

## 9. Anti-drift contract

If at any point you find yourself wanting to:

- Decode `.uasset` content (parse mesh / texture bytes) → STOP. Phase 2B.
- Compute any kind of hash (FNV, CRC32C, MD5, etc.) → STOP. Phase 2C.
- Build a `PreparedAssetInventory` from extracted files → STOP. Phase 2C.
- Modify `PreparedAssetInventory`, `ExtractedAssetRecord`, or any schema in `src/domain/mod.rs` → STOP, report blocker.
- Touch `compare/`, `inference/`, `proposal/`, `human_summary/`, `report_storage/`, `report/`, `gui_app/`, `bin/gui.rs` → STOP, report blocker.
- Add any new Cargo dependency beyond `repak` → STOP, report blocker.
- Add a new `#[cfg(test)]` block inside `src/` → STOP, report blocker.
- Try to "auto-fetch" AES keys from a remote endpoint → STOP. Out of scope for Phase 2A.
- Invent a new schema_version that is NOT `whashreonator.pak-extraction-manifest.v1` → STOP, the schema name is locked by this spec.
- Implement repack / write-back of pak files → STOP, never needed.
- Print, log, or persist the AES key value anywhere except the encrypted memory where `repak` consumes it → STOP, this is a security violation.
- Improve unrelated code you happen to read while working → STOP, leave it. Stay surgical.

## 10. Blocker handling

Per the same playbook as Phase 1:

1. Stop coding.
2. Document the blocker concretely (file/line, what you tried, what conflicts).
3. Estimate impact: blocks all of Phase 2A, or just one branch?
4. Continue with whatever you CAN do that does not depend on the blocker.
5. List both completed work and the blocker in the final report.

DO NOT fake success. DO NOT silently pivot to "subprocess CUE4Parse" if `repak` doesn't work — that is a strategy decision the project owner makes, not you.

## 11. Final report template (Vietnamese, MANDATORY)

Same structure as Phase 1 final report. Adjust section 2 (Việc đã làm) and section 8 (Anti-scope) to reflect Phase 2A scope. Keep all sections.

```markdown
## Báo cáo Phase 2A — Pak extractor

### 1. Tình trạng tổng quát
- **Phần trăm hoàn thành**: <X>% (dựa trên acceptance checklist mục 13 trong `docs/phase2a-pak-extractor.md`)
- **Trạng thái build**: <pass | fail>
- **Trạng thái test**: <X passed / Y total>
- **Có blocker không**: <yes | no>

### 2. Việc đã làm
- `Cargo.toml` — thêm dep `repak`, version <X>
- `src/ingest/pak_extractor.rs` — <mô tả ngắn>
- `src/cli/mod.rs` — variant `ExtractPak` + struct args
- `src/lib.rs` — dispatcher arm
- `src/pipeline/mod.rs` — `run_extract_pak_command`
- `tests/pak_extractor.rs` — <số test, tên test>

### 3. Việc chưa làm và lý do
<list>

### 4. Blocker (nếu có)
<list with concrete file/line, spec citation, conflict, attempted workaround, suggestion>

### 5. Test đã chạy
- `cargo fmt`
- `cargo build --lib`
- `cargo build --bin whashreonator`
- `cargo test --test pak_extractor`
- `cargo test` (full)

### 6. End-to-end smoke test
<command run, exit code, output dir contents, manifest schema version>

### 7. Real-root verification
- Lệnh trong Phase 2A có thể đọc từ `C:\Wuthering Waves\Wuthering Waves Game`? <yes — pak files only, read-only>
- Lệnh trong Phase 2A có thể ghi vào `C:\Wuthering Waves\Wuthering Waves Game`? <no, never>
- Xác nhận: nothing was written into the real game root: <yes | no>
- Xác nhận: AES key never logged, never persisted, never echoed: <yes | no>

### 8. Anti-scope confirmation
- [ ] Không decode `.uasset` content
- [ ] Không tính hash (FNV / CRC32C / MD5)
- [ ] Không build `PreparedAssetInventory`
- [ ] Không sửa schema struct trong `src/domain/mod.rs`
- [ ] Không touch compare/inference/proposal/storage/GUI
- [ ] Không thêm dep mới ngoài `repak`
- [ ] Không thêm `#[cfg(test)]` mới trong `src/`
- [ ] Không print / log / persist AES key

### 9. Đề xuất review
- File reviewer nên đọc đầu tiên: `src/ingest/pak_extractor.rs`
- Phần code reviewer nên xem kỹ nhất: <path:line-range, reason>
- Test case nào reviewer nên chạy thủ công: <name>

### 10. Câu hỏi cần user trả lời (nếu có)
<list, hoặc "không có">
```

## 12. Done definition

Phase 2A is done when:

1. All 9 items in section 13 acceptance checklist are ticked.
2. The Vietnamese final report (section 11) is delivered to the user.
3. Working tree is clean OR you explicitly tell the user "I left changes uncommitted for your review".

If any of these is not true, Phase 2A is NOT done — even if all tests pass.

## 13. Acceptance checklist

Tick each before declaring Phase 2A done:

- [ ] `cargo fmt` clean.
- [ ] `cargo build --lib` clean.
- [ ] `cargo build --bin whashreonator` clean.
- [ ] All 8-9 tests in `tests/pak_extractor.rs` pass.
- [ ] Full `cargo test` suite still green (no regression).
- [ ] No new `#[cfg(test)]` block inside `src/`.
- [ ] CLI help text for `extract-pak` mentions read-only `--pak`, AES key flag exclusivity, and validate output dir behavior.
- [ ] Smoke test (synthetic unencrypted fixture) succeeds.
- [ ] Real-root verification confirmed in final report.

## 14. Cross-references

- `src/cli/mod.rs` — existing 11 subcommands as pattern reference. Closest analog: `Command::Snapshot`.
- `src/pipeline/mod.rs` — existing `run_*_command` helpers as pattern reference. Closest analog: `run_snapshot_command` and `run_ingest_frame_analysis_command` (Phase 1).
- `src/output_policy/mod.rs` — `validate_artifact_output_path` (mandatory call before any output write).
- `src/error.rs` — `AppError`, `AppResult` types.
- `tests/frame_analysis_ingest.rs` — closest analog for new tests in `tests/pak_extractor.rs` (CLI command end-to-end with tempdir, schema round-trip, output-policy reject).
- `docs/phase1-fa-adapter.md` — Phase 1 spec; Phase 2A follows the same structural conventions.

## 15. External references

- `repak` Rust crate: https://github.com/trumank/repak — UE4 pak format reader/writer in Rust, supports UE4.26-5.3 with AES read.
- WuWa AES key archive (community, public): https://github.com/ClostroOffi/wuwa-aes-archive — reference only; NEVER commit a key into our repo.
- WWMI mod tools (modder workflow context, not directly used by Phase 2A): https://github.com/SpectrumQT/WWMI-TOOLS

## 16. Anti-scope reminder (final)

Phase 2A is the **simplest** of Phase 2's three sub-phases:
- 2A: pak → files on disk (THIS phase).
- 2B: files on disk → mesh data structs (next phase, separate spec).
- 2C: mesh data → runtime hashes → snapshot integration (final phase, separate spec).

Resist the temptation to "just throw in mesh decoding while we're here". The discipline of staying inside one sub-phase is what keeps the project shippable on each step.

End of Phase 2A spec.
