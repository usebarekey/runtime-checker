# runtime-checker
Detects the minimum version needed for **Node.js**, **Bun**, **Deno** and browsers to execute your codebase.

## Installation
```sh
deno add npm:runtime-checker
bun add runtime-checker
pnpm add runtime-checker
npm install runtime-checker
```

## Usage
`$ runtime-checker <dir>`

### Arguments
 `--fast:` uses [fff](https://github.com/dmtrKovalenko/fff) instead of oxc AST parsing. Faster<sup>1</sup>, but less accurate.
 
 <sup>1</sup> fff is only faster (and actually slower!) than oxc when your codebase is around 250,000~ or more lines of code, from testing on a Windows machine with a 9800X3D.

 `--fix`: Automatically fixes your `engines.node` field to a supported version if an issue is found.

 `--inspect <symbol, e.g.: Symbol.asyncDispose>`: Shows each detection of a specific symbol

 `--summary`: Only prints the summary

 `--runtime <all|deno|bun|node|safari|chrome|firefox>`: Only shows the results for a specific runtime
