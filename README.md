# runtime-checker

Find the minimum runtime version needed for a JavaScript or TypeScript codebase.

It parses files with Oxc and checks runtime APIs, JavaScript syntax, module format, and native TypeScript usage. It can report requirements for Node.js, Deno, Bun, Safari, Chromium, and Firefox.

## install

```sh
npm install runtime-checker
pnpm add runtime-checker
bun add runtime-checker
deno add npm:runtime-checker
```

## usage

```sh
runtime-checker <dir>
```

For app compatibility, scan the code you ship. For bundled apps, that usually means the build output, not just `src`.

## options

```sh
runtime-checker <dir> --summary
runtime-checker <dir> --runtime node
runtime-checker <dir> --inspect Symbol.asyncDispose
runtime-checker <dir> --fix
```

- `--summary` prints only the result panel.
- `--runtime <all|node|deno|bun|safari|chrome|firefox>` limits output to one target.
- `--inspect <feature>` prints every detection for a feature.
- `--fix` updates `package.json` `engines.node` when the detected Node.js version is not satisfied.

## detects

- Runtime APIs such as `fetch`, `Temporal`, `fs.cp`, `sqlite.DatabaseSync`, and `Symbol.asyncDispose`.
- JavaScript syntax such as optional chaining, nullish coalescing, ESM, `await using`, and native TypeScript support.
- Node.js `engines.node` mismatches.

## output

```txt
Finished in 665ms using oxc (ast parsing) after scanning 307k lines of code.

Runtimes
- Node.js 24.0.0
- Deno 2.8.0
- Bun 1.3.0

Browsers
- Safari 26.0.0
- Chromium 149.0.0
- Firefox 141.0.0
```

Use `--inspect <feature>` when you want to see every file and location that caused a requirement.
