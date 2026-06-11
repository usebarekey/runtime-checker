import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const exe = process.platform === "win32" ? "runtime-checker.exe" : "runtime-checker";
const native = join(root, "npm", "bin", "native", exe);

if (existsSync(native)) {
  process.exit(0);
}

const cargo = spawnSync("cargo", ["build", "--release"], {
  cwd: root,
  stdio: "inherit",
});

if (cargo.status !== 0) {
  console.error("Failed to build runtime-checker. Install Rust from https://rustup.rs/ and retry.");
  process.exit(cargo.status ?? 1);
}

const prepare = spawnSync(process.execPath, ["./npm/prepare-bin.js"], {
  cwd: root,
  stdio: "inherit",
});

process.exit(prepare.status ?? 0);
