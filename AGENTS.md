# Repository Guidelines

## Project Structure & Module Organization
`src/` contains the Rust application code. `src/main.rs` is the CLI entry point, `src/bin/gui.rs` builds the GUI wrapper, and feature modules such as `pipeline/`, `snapshot/`, `inference/`, `proposal/`, and `wwmi/` hold the domain logic. `tests/` contains integration-style tests, with JSON fixtures under `tests/fixtures/`. Build outputs land in `target/` and should stay untracked. Runtime artifacts from release builds must be written under `release/`; debug/dev artifacts may be written under `out/`.

## Build, Test, and Development Commands
Use Cargo for all local workflows:

- `cargo build` builds the CLI and library.
- `cargo build --release` builds optimized CLI and GUI binaries.
- `cargo run -- --help` lists CLI commands and flags.
- `cargo run -- snapshot --source-root .\game-snapshots\2.4.0 --version-id 2.4.0 --output .\release\snapshot-2.4.0.json` runs a representative pipeline command using the release artifact policy.
- `cargo test` runs the full integration test suite.
- `cargo test pipeline_mvp` runs a focused test while iterating.
- `cargo fmt` formats the codebase; run before submitting changes.

## Coding Style & Naming Conventions
Follow standard Rust formatting with 4-space indentation and `cargo fmt` as the source of truth. Prefer `snake_case` for modules, files, functions, and test names; use `PascalCase` for types and enums; keep CLI flags long-form and descriptive, such as `--compare-report` or `--min-confidence`. Keep modules narrow and place command argument structs in `src/cli/mod.rs`.

## Testing Guidelines
Add or update integration tests in `tests/` for user-visible behavior and pipeline regressions. Name test files by feature, such as `snapshot_mode.rs` or `proposal_mode.rs`, and name test functions for the expected outcome, such as `pipeline_exports_expected_json_report`. Reuse fixtures under `tests/fixtures/` when validating stable JSON shapes. Run `cargo test` before opening a PR.

## Commit & Pull Request Guidelines
This repository currently has no published commit history, so no established commit convention can be inferred yet. Use short, imperative commit subjects such as `Add snapshot diff summary output`. Keep commits focused on one concern. PRs should include a concise description, affected commands or modules, test coverage notes, and sample output or screenshots when CLI/GUI behavior changes.

## Security & Configuration Tips
Do not commit local game folders, generated reports, or cloned upstream repos. Keep large artifacts under `release/` for release runs (or `out/` for debug/dev runs) and never write generated artifacts into source directories such as `src/`, `tests/`, or the repository root. When testing commands that read local installs, prefer explicit output paths so generated JSON and Markdown stay isolated from source files.
