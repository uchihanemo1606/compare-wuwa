# Sample Frame Analysis fixture

Synthetic but format-accurate sample of a 3DMigoto FrameAnalysis log, derived from
the documented format used by `bo3b/3Dmigoto` and the reference parser in
`leotorrez/XXMITools` (`migoto/datastructures.py::FALogFile`).

This fixture is **not** captured from a real WuWa play session. It is hand-written
to exercise the parser line patterns. When a real WWMI dump becomes available, add
it as a separate fixture (do not replace this one) so format coverage stays explicit.

## What this fixture exercises

- 4 draw calls (`000123` through `000126`) with realistic numbering.
- `IASetVertexBuffers` with multiple slots and single slot.
- `IASetIndexBuffer` with `R32_UINT` and `R16_UINT` formats.
- `VSSetShader` / `PSSetShader` resource bindings.
- `DrawIndexed` calls with varying `IndexCount` values.
- `SOSetTargets` (stream output) — present for parser robustness even though Phase 1
does not need to consume it for hash extraction.
- One bind line that includes the optional `view=0x...` prefix
(`view=0xDEADBEEF resource=...`).
- Repeated VB hash (`4a7d9c1f` appears in draw calls `000123` and `000126`) so the
parser's deduplication logic is testable.
- Repeated VS hash (`aabbccdd`) across draw calls 123 and 124 — same vertex shader
used for two meshes.

## Line format reference (from XXMITools `FALogFile`)


| Line kind         | Pattern                                                                                                              |
| ----------------- | -------------------------------------------------------------------------------------------------------------------- |
| Drawcall API line | `^(?P<drawcall>\d+)` then API name and parens                                                                        |
| Resource bind     | `^\s+(?P<slot>[0-9D]+): (?:view=(?P<view>0x[0-9A-F]+) )?resource=(?P<address>0x[0-9A-F]+) hash=(?P<hash>[0-9a-f]+)$` |
| Header            | First line is `analyse_options=...`                                                                                  |


## Hash format observation

- Lowercase hex.
- Variable length: usually 8 chars for buffer hashes (`4a7d9c1f`), can be longer.
- WWMI mod INI files use the same hex string as the `hash =` value.

## Source attribution

- 3DMigoto FrameAnalysis behaviour: [https://github.com/bo3b/3Dmigoto](https://github.com/bo3b/3Dmigoto)
- Reference parser used to derive patterns: [https://github.com/leotorrez/XXMITools/blob/main/migoto/datastructures.py](https://github.com/leotorrez/XXMITools/blob/main/migoto/datastructures.py)
- Tutorial that confirms hash hex format: [https://leotorrez.github.io/modding/guides/hunting](https://leotorrez.github.io/modding/guides/hunting)