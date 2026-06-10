# MDN Browser Compatibility Data Source

This directory is generated from `@mdn/browser-compat-data`.

MDN does not expose this as a query API. The maintained machine-readable source is the `@mdn/browser-compat-data` npm package, which also publishes a raw `data.json` file through package CDNs.

The pinned BCD version is stored in `data/mdn-bcd.version`.

Regenerate from the pinned CDN release:

```sh
cargo run --bin generate-mdn-data -- --output-dir data/mdn
```

Regenerate from an already downloaded `data.json` file:

```sh
cargo run --bin generate-mdn-data -- --input path/to/data.json --output-dir data/mdn
```

The generator emits:

- `node.ron` from BCD runtime id `nodejs`
- `deno.ron` from BCD runtime id `deno`
- `bun.ron` from BCD runtime id `bun`
- `safari.ron` from BCD runtime id `safari`
- `chrome.ron` from BCD runtime id `chrome`
- `firefox.ron` from BCD runtime id `firefox`

Important: this is a source layer for JavaScript builtins and Web APIs. It does not cover Node core modules such as `fs`, `path`, or `http`; those need a separate Node documentation source layer before replacing the main `data/node.ron`.
