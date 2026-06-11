#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const exe = process.platform === "win32" ? "runtime-checker.exe" : "runtime-checker";
const native = join(root, "npm", "bin", "native", exe);

if (!existsSync(native)) {
  console.error("runtime-checker native binary is missing. Try reinstalling the package.");
  process.exit(1);
}

const child = spawnSync(native, process.argv.slice(2), { stdio: "inherit" });

if (child.error) {
  console.error(child.error.message);
  process.exit(1);
}

process.exit(child.status ?? 0);
