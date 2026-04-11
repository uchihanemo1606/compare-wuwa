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
Add or update focused tests when behavior changes in snapshot capture, compare, report, continuity, inference, or storage. Prefer integration tests in `tests/` for user-visible behavior and storage/report flows; use module tests when the logic is narrow and self-contained.

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
