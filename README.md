<h1 align="center">runtime-checker</h1>

<p align="center">
  <a href="https://www.npmjs.com/package/runtime-checker">npm</a>
  &bull;
  <a href="https://docs.barekey.dev/runtime-checker">docs</a>
</p>

---

Turn the APIs and syntax in a JavaScript or TypeScript codebase into a concrete runtime floor.

```console
$ npx runtime-checker .

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

runtime-checker parses JavaScript and TypeScript with Oxc, then combines runtime APIs, language syntax, module format, and native TypeScript usage into minimum versions for Node.js, Deno, Bun, Safari, Chromium, and Firefox.

Narrow a scan with `--runtime node`, print only the result with `--summary`, or trace every location behind a requirement with `--inspect Symbol.asyncDispose`. `--fast` uses less precise text matching. Directories named `.git`, `node_modules`, `dist`, `build`, `coverage`, and `target` are skipped.

Visit the **[documentation](https://docs.barekey.dev/runtime-checker)** for installation, command options, detection details, and limitations.
