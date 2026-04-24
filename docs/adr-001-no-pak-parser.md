# ADR-001: No Rust `.pak` parser for Wuthering Waves

Status: **Accepted** (2026-04-21)

## Context

Phase 2A attempted to ship a Rust-based `.pak` extractor for Wuthering Waves (WuWa) game archives. The goal was to obtain offline ground-truth asset data without requiring the user to launch the game. We vendored `trumank/repak` v0.2.3, added `aes = "0.8.4"` as a direct dependency, and patched repak to support GUID-based key resolution (`Key::Resolver`).

Real-pak verification against `C:\Wuthering Waves\Wuthering Waves Game\Client\Content\Paks\*.pak` (WuWa 3.2.2) produced a consistent failure mode: `io error: failed to fill whole buffer`, even after successful AES decryption of the pak index.

Isolated byte-level investigation (via a throwaway `debug_decrypt` binary) confirmed that:

1. The AES-256 key from community archives (`555me/wuwa-keys` `mainKey`) is **correct**. Decrypting the first 16 bytes of the encrypted index produces the expected FString layout (length=46, mount point `../../../Client/Content/Aki/ScriptAssemblies/`).
2. The pak footer, index header, `PathHashIndex`, and `FullDirectoryIndex` all parse correctly with repak's existing V11 code path applied to WuWa's V12 footer.
3. The **encoded entry records** (which encode per-file offset/size/compression) do **not** match the Unreal Engine 4.27 / 5.0 reference layout. The first encoded entry for `CSharpScript.dll` parsed as `offset = 1,232,994,308` in a 104,636,189-byte file — clearly invalid.

Cross-check against the `CUE4Parse` C# project (the parser used by FModel) revealed a WuWa-specific encoded-entry branch:

```csharp
if (reader.Game == GAME_WutheringWaves && reader.Info.Version > PakFile_Version_Fnv64BugFix)
{
    bitfield = (bitfield >> 16) & 0x3F | (bitfield & 0xFFFF) << 6
             | (bitfield & (1 << 28)) >> 6 | (bitfield & 0x0FC00000) << 1
             | (bitfield & 0xC0000000) >> 1 | (bitfield & 0x20000000) << 2;
    CustomData = Ar.Read<int>();
}
// ...
(Offset, UncompressedSize) = (UncompressedSize, Offset);
```

WuWa applies three non-standard transforms on every encoded entry:

- **bitfield shuffle** across multiple bit ranges
- **extra `CustomData` field** (4 bytes) not present in the UE reference format
- **swap** of the `Offset` and `UncompressedSize` fields after reading

Empirically, the observed encoded-entry stride in WuWa 3.2.2 (13 bytes per entry) does not match what the published CUE4Parse transform expects (~16 bytes), suggesting WuWa has further evolved the layout beyond what CUE4Parse currently handles.

## Decision

**We will not ship a Rust `.pak` parser for Wuthering Waves.**

This means:

- Remove the vendored `repak` copy under `vendor/repak/`.
- Remove the `repak` and direct `aes` dependencies from `Cargo.toml`.
- Delete `src/ingest/pak_extractor.rs`, `tests/pak_extractor.rs`, the `extract-pak` CLI command, and `run_extract_pak_command`.
- Treat pak-format reverse engineering for WuWa as **out of scope** for this project going forward.

## Consequences

### Positive

- The tool depends only on **runtime hashes** from 3DMigoto / WWMI Frame Analysis dumps (Phase 1 FA adapter). Runtime hashes are the canonical oracle because they are exactly what WWMI evaluates against mod `.ini` files at draw time; if the runtime hash matches, the mod works. No offline decoding chain (pak → uasset → mesh → hash) can be more accurate.
- The backend cannot be broken by Kuro modifying pak obfuscation in a future game patch.
- Build time and binary size stay small. Dependency surface stays free of cryptographic and compression stacks that were only needed for pak reading.
- Clear architectural boundary: **game-format parsing is Kuro's moving target, not ours.**

### Negative

- Automation target drops from Level 4 (game-update → fix, fully offline) to **Level 3** (user runs the game briefly with WWMI once per patch, tool does the rest). Measured against the 2-week manual WWMI fix cycle, a user-in-the-loop workflow that takes ~30 minutes is still a very large improvement.
- Pre-patch "offline diff" use cases (analyzing a downloaded patch before launching the game) are not supported by this project directly.

### Residual options, not pursued now

- **Option A — port CUE4Parse WuWa quirks to Rust.** Rejected. The WuWa pak format is a moving target maintained by a studio we have no coordination with. A single-person maintainer cannot sustainably chase monthly format changes, and even CUE4Parse's published transforms appear to lag behind WuWa 3.x.
- **Option B — shell out to FModel / CUE4Parse CLI.** Deferred. If an offline "pre-game" analysis workflow is ever needed, we will integrate the existing community tool as an external process rather than reimplementing it. This keeps reverse-engineering effort with the community that already does it.

## Alternatives considered (decision matrix)

| Option | Effort | Maintenance risk | Accuracy vs runtime | Depends on | Decision |
|---|---|---|---|---|---|
| A. Rust port of CUE4Parse WuWa quirks | 3–5 days + ongoing | Very high (patch cadence ~1 month) | Indirect (chain of decoders) | Our own reverse engineering | Rejected |
| B. Shell out to FModel / CUE4Parse CLI | ~1 day | Low (community maintains) | Indirect (still offline) | .NET runtime, external tool | Deferred |
| **C. Frame Analysis only (adopted)** | 0 extra | None on our side | **Direct (runtime == mod contract)** | 3DMigoto / WWMI | **Adopted** |

## References

- `src/ingest/frame_analysis.rs` — Phase 1 FA adapter.
- `docs/phase1-fa-adapter.md` — Phase 1 specification.
- `tests/fixtures/sample_frame_analysis/` — synthetic 3DMigoto log fixture.
- CUE4Parse `FPakEntry.cs` — WuWa-specific encoded-entry handling (external reference, not vendored).

## Cost of this investigation

Approximately 1.5 days of Codex-assisted implementation + reverse engineering to reach a decisive "not our problem" verdict. The vendored `repak` and pak-extractor modules have been deleted; the investigation survives as this ADR so future phases do not repeat the mistake.
