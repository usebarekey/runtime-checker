# MDN Browser Compatibility Data Source

This directory is generated from `@mdn/browser-compat-data`.

MDN does not expose this as a query API. The maintained machine-readable source is the `@mdn/browser-compat-data` npm package, which also publishes a raw `data.json` file through package CDNs.

The pinned BCD version is stored in `data/mdn-bcd.version`.

Generate JSON artifacts from the pinned CDN release:

```sh
python scripts/generate_mdn_data.py
```

Generate JSON artifacts from an already downloaded `data.json` file:

```sh
python scripts/generate_mdn_data.py --input path/to/data.json
```

Generated JSON is written to `data/generated/mdn/` and is intentionally ignored by git.

Rebuild the checked-in Rust static cache after generating JSON:

```sh
python scripts/generate_runtime_data.py
cargo fmt
```

The generator emits:

- `node.ron` from BCD runtime id `nodejs`
- `deno.ron` from BCD runtime id `deno`
- `bun.ron` from BCD runtime id `bun`
- `safari.ron` from BCD runtime id `safari`
- `chrome.ron` from BCD runtime id `chrome`
- `firefox.ron` from BCD runtime id `firefox`

The generated artifacts include JavaScript builtins, Web APIs, and JavaScript syntax compatibility entries under `syntax.*`.

Important: this is a source layer for JavaScript builtins, Web APIs, and syntax compatibility. It does not cover Node core modules such as `fs`, `path`, or `http`; those need a separate Node documentation source layer before replacing the main `data/node.ron`.
