import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const exe = process.platform === "win32" ? "runtime-checker.exe" : "runtime-checker";
const target = `${process.platform}-${process.arch}`;

if (existsSync(join(root, "npm", "bin", "native", target, exe))) {
  process.exit(0);
}

console.error(`runtime-checker does not have a native binary for ${target}.`);
process.exit(1);
