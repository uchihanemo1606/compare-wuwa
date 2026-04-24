# Real-format frame analysis fixture

Synthetic but **format-faithful** to live WWMI / 3DMigoto Frame Analysis dumps
captured from Wuthering Waves at runtime. Differs from
`tests/fixtures/sample_frame_analysis/log.txt` in three ways that the parser
must handle:

1. Header line uses `analyse_options: <hex flags>` (colon + hex value), not
   `analyse_options=<flag list>`.
2. `IASetIndexBuffer`, `VSSetShader`, and `PSSetShader` carry the resource
   `hash=<hex>` token **inline at the end of the API line**, after the closing
   `)`, instead of on an indented `resource=... hash=...` continuation line.
3. `IASetIndexBuffer` `Format:` argument uses the raw DXGI numeric value
   (`Format:42` = `DXGI_FORMAT_R32_UINT`, `Format:57` = `DXGI_FORMAT_R16_UINT`)
   instead of the symbolic name.
4. `VSSetShader` / `PSSetShader` API lines include the extra parameters
   `ppClassInstances` and `NumClassInstances` (real D3D11 signature), which
   the parser must skip without confusion.

The fixture also includes a `PSSetConstantBuffers` call (an API the parser
treats as ignored) followed by an indented continuation line, to confirm the
parser does not crash on ignored-API continuation lines.

Hash values, resource addresses, and slot numbers are synthetic. Real dumps
contain actual game-side hashes and addresses, and are not committed to this
repository.
