# Repository Guidelines

## Project Mission
This repository exists to help the WWMI team understand how Wuthering Waves updates break existing mods and to support later repair work. The core path is:

1. capture rich game-version data,
2. compare versions conservatively,
3. build trustworthy continuity/history across updates,
4. only then use WWMI knowledge to assist repair decisions.

Keep the product focused on `analyze / diff / infer / assist repair` for existing mods. Do not drift into mod-authoring features, generic mod cataloging, or UI work unrelated to breakage analysis.

## Project Structure & Module Organization
`src/` contains the Rust application code. `src/main.rs` is the CLI entry point, and `src/bin/gui.rs` builds the GUI wrapper. The most important domain modules are:

- `snapshot/` for capture and normalization of game asset data
- `compare/` for pairwise version diffing, lineage, and remap safety
- `report/` for human-readable and machine-readable diff views, including continuity summaries
- `report_storage/` for saved snapshots, reports, and continuity loading
- `inference/` and `proposal/` for downstream repair assistance
- `wwmi/` for WWMI-specific knowledge, which is secondary until the version-diff foundation is strong

`tests/` contains integration-style tests, with fixtures under `tests/fixtures/`. Build outputs land in `target/` and should stay untracked.

Artifact policy:

- debug/dev flows may write under repo-local `out/`
- release app flows should write relative to the executable, under a sibling `report/` directory
- never write generated artifacts into `src/`, `tests/`, or the repository root

## Build, Test, and Development Commands
Use Cargo for all local workflows:

- `cargo build` builds the CLI and library
- `cargo build --release` builds optimized CLI and GUI binaries
- `cargo run -- --help` lists CLI commands and flags
- `cargo test` runs the full test suite
- `cargo test prepared_mesh_buffer_flow -- --test-threads=1` runs a focused prepared-data regression
- `cargo test --lib continuity_index_ -- --test-threads=1` runs focused continuity tests
- `cargo fmt` formats the codebase; run it before submitting changes

## Coding Style & Naming Conventions
Follow standard Rust formatting with 4-space indentation and `cargo fmt` as the source of truth. Prefer `snake_case` for modules, files, functions, and test names; use `PascalCase` for types and enums. Keep CLI flags long-form and descriptive, such as `--compare-report` or `--min-confidence`.

Favor additive, inspectable data models. When changing schemas, preserve backward compatibility with `#[serde(default)]` where appropriate. Prefer conservative classification and review-oriented outcomes over optimistic auto-merging when evidence is weak or ambiguous.

## Testing Guidelines
Add or update focused tests when behavior changes in snapshot capture, compare, report, continuity, inference, or storage.

Hard rule for test placement:

- All new or updated tests must be placed in `tests/*.rs`.
- Do not add new inline `#[cfg(test)]` blocks in backend logic files under `src/`.
- Do not expand existing inline test blocks in backend logic files under `src/`.
- If a change touches logic that already has inline tests, move the touched/added test coverage into `tests/*.rs` as part of the same phase whenever practical.

Good test targets include:

- prepared asset signals propagating through snapshot and compare
- lineage and remap compatibility remaining conservative
- continuity staying sequence-safe across multiple versions
- output/storage behavior for debug vs release app flows

Reuse fixtures under `tests/fixtures/` when validating stable JSON shapes. Run `cargo test` before opening a PR.

## Commit & Pull Request Guidelines
This repository currently has no published commit history, so no established commit convention can be inferred yet. Use short, imperative commit subjects such as `Add continuity milestone summaries` or `Fix release report root selection`. Keep commits focused on one concern.

PRs should include:

- a concise statement of the version-diff or repair-support gap being addressed
- affected commands or modules
- test coverage notes
- sample output or screenshots when CLI/GUI behavior changes

## Security & Configuration Tips
Do not commit local game folders, generated reports, cloned upstream repos, or private mod collections. Keep generated artifacts isolated under `out/` for dev/debug runs or the executable-relative `report/` folder for release app runs. When testing commands that read local installs or prepared asset dumps, prefer explicit paths so generated JSON and Markdown stay isolated from source files.

## Codex Standing Rules
Codex should treat the following as always-on repository rules:

- Read code and tests first. Do not use README files as the source of truth.
- Report back to the user in Vietnamese unless the user explicitly asks for another language.
- Keep technical identifiers, code symbols, file paths, commands, and literal program output in their original language.
- Do not drift into mod-authoring, mod packaging, or generic mod-management features. Stay focused on version diff, evidence gathering, continuity, repair-assist, and reviewer-facing outputs.
- Prefer additive changes, conservative review-first decisions, and backward-compatible schema evolution.
- **Pak-format reverse engineering for Wuthering Waves is out of scope.** Do not reintroduce a Rust `.pak` parser, do not vendor `repak` (or any equivalent), and do not add direct dependencies on `aes`/`oodle`/Unreal-pak crates for the purpose of offline `.pak` decoding. The canonical ground truth for runtime hashes is the Frame Analysis (3DMigoto / WWMI) adapter. See `docs/adr-001-no-pak-parser.md` for context and rationale.

## Backend Phase Defaults
When Codex is asked to implement a backend phase, these defaults apply unless the user explicitly overrides them:

- Keep the phase backend-only. Do not expand into GUI work unless the user explicitly asks for it.
- Prefer the smallest high-leverage slice that fits the named phase instead of broad speculative architecture work.
- Reuse existing backend modules where possible, especially `snapshot/`, `compare/`, `report/`, `report_storage/`, `inference/`, `proposal/`, `human_summary/`, and `wwmi/dependency.rs`.
- Keep all new or updated tests in `tests/*.rs` (no new inline `#[cfg(test)]` in backend logic modules under `src/`).
- If an optional real-data branch is unavailable (for example a real extractor inventory or a real changed compare pair), do not block the whole phase. Implement the backend hardening that can be done now, verify with fixtures or currently available artifacts, and report the limitation explicitly instead of faking success.

## Real Game Root Verification
After each implementation phase and automated test pass, Codex must run a real-data verification pass against the real game root:

`C:\Wuthering Waves\Wuthering Waves Game`

This verification is mandatory unless the user explicitly disables it.

Strict safety rules:

- Treat the real game root as read-only input only.
- Do not modify, rename, delete, or write any file inside the game directory.
- Do not create outputs, temp files, or reports inside the game directory.
- All outputs must go only to approved artifact locations such as repo-local `out/`, report storage roots, or executable-relative release/report locations.
- If any command or code path would write into the game root, stop and correct it before proceeding.

Every final implementation report should include:

- which real-root verification command(s) were run
- which output path(s) were produced
- what happened on real data
- explicit confirmation that nothing was written into the game root

## Real Mod Directories
If a phase uses real mod directories such as `D:\mod\WWMI\Mods`, Codex must treat them as read-only input only.

- Do not modify, rename, delete, or write any file inside real mod directories.
- Any outputs derived from real mod directories must go only to approved artifact locations such as repo-local `out/` or report storage roots.
- Final implementation reports should explicitly confirm that nothing was written into the real mod directories when they were used.
