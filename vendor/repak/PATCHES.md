# Local patches against upstream repak

Upstream source: https://github.com/trumank/repak (tag `v0.2.3`)

This file lists every local modification made to the vendored repak source.
Each entry MUST be re-applied if the vendored copy is refreshed from a newer
upstream release. Keep diffs minimal and reviewer-friendly.

## Patch 1 — Suppress `unstable_name_collisions` warning in `entry.rs`

- **File**: `repak/src/entry.rs`
- **Line**: ~134 (inside `Entry::read` body)
- **Change**: rewrite `reader.read_array(Block::read)` as
  `ReadExt::read_array(reader, Block::read)` (fully qualified syntax).
- **Reason**: avoids the `unstable_name_collisions` warning when std lib may
  add a `read_array` method to `Read` in the future. Behaviour is identical;
  this is a Rust syntax disambiguation only.
- **Compatibility**: when upstream merges or independently fixes this same
  warning, drop this patch.
