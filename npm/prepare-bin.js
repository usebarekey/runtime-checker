import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const exe = process.platform === "win32" ? "runtime-checker.exe" : "runtime-checker";
const source = join(root, "target", "release", exe);
const target = join(root, "npm", "bin", "native", `${process.platform}-${process.arch}`, exe);

mkdirSync(dirname(target), { recursive: true });
copyFileSync(source, target);
