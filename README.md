# runtime-checker

Detect the minimum runtime versions required by a JavaScript or TypeScript codebase.

```bash
runtime-checker <dir>
runtime-checker <dir> --fast
runtime-checker <dir> --summary
runtime-checker <dir> --runtime node
```

The default scanner uses Oxc AST parsing. `--fast` uses the FFF-backed text scanner and can report false positives from comments, strings, or shadowed local names.

## Install

```bash
cargo install runtime-checker
```

The npm package builds the Rust binary during install:

```bash
npm install -g runtime-checker
```

That path requires a local Rust toolchain. Prebuilt npm binaries can be added later without changing the CLI command.

## License

BSD-3-Clause.
