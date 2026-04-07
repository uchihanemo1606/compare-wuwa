# WhashReonator Release Guide

## 1. Prepare WWMI Knowledge

Generate or refresh the WWMI reference knowledge JSON before comparing versions:

```powershell
cargo run -- extract-wwmi-knowledge `
  --repo .\_upstream\WWMI-Package `
  --max-commits 200 `
  --output .\out\wwmi-knowledge.json
```

Use a reviewed local clone when possible. The tool uses WWMI history as supporting evidence only; it does not assume WWMI is the source of truth for the game.

## 2. Build Release Binaries

```powershell
cargo build --release
```

This produces:

- `target\release\whashreonator.exe`
- `target\release\gui.exe`

In release builds, all generated artifacts must be written under `release\`. The app now enforces this output policy.

## 3. Run the Desktop App

Launch:

```powershell
.\target\release\gui.exe
```

In the `Compare` screen:

1. Enter `old` and `new` game roots.
2. Enter explicit version ids for each root.
3. Optionally point to `release\wwmi-knowledge.json`.
4. Click `Run Compare`.

The desktop flow creates managed bundles under `release\reports\<old>-to-<new>-<timestamp>\` containing:

- `report.v2.json`
- `old.snapshot.json`
- `new.snapshot.json`
- `compare.v1.json`
- `inference.v1.json` when WWMI knowledge is provided

## 4. Review Reports

Open the `Report Manager` tab to:

- list saved reports
- search by version/path
- filter by old version, new version, or resonator
- reopen an older report bundle

The v2 report shows old/new version columns through resonator-scoped items:

- assets
- buffers
- mapping candidates
- status: `unchanged`, `changed`, `added`, `removed`, `uncertain`
- confidence and reasons when the tool proposes a mapping-related change

## 5. CLI Compatibility

Existing CLI modes remain valid. For debug/dev work, continue using `out\`. Examples:

```powershell
cargo run -- snapshot --source-root .\game-old --version-id 2.4.0 --output .\out\snapshot-2.4.0.json
cargo run -- compare-snapshots --old-snapshot .\out\snapshot-2.4.0.json --new-snapshot .\out\snapshot-2.5.0.json --output .\out\snapshot-compare.json
```

Do not write artifacts into `src\`, `tests\`, or the repository root.

## 6. Verification Before Shipping

Run:

```powershell
cargo test
cargo build --release
```

Smoke-check one real compare run from the GUI and confirm a new bundle appears under `release\reports\`.
