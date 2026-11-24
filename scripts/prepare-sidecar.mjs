// Builds `velox-host` and copies it to where Tauri's `externalBin` expects it:
// apps/desktop/src-tauri/binaries/velox-host-<target-triple>[.exe]
import { execSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const release = process.argv.includes("--release") || process.env.TAURI_ENV_DEBUG === "false";
const target =
  process.env.TAURI_ENV_TARGET_TRIPLE ||
  process.argv.find((a) => a.startsWith("--target="))?.slice(9) ||
  execSync("rustc -vV").toString().match(/host: (\S+)/)[1];
const ext = target.includes("windows") ? ".exe" : "";

const args = ["build", "-p", "velox-host", "--target", target];
if (release) args.push("--release");
execSync(`cargo ${args.join(" ")}`, { cwd: root, stdio: "inherit" });

const src = join(root, "target", target, release ? "release" : "debug", `velox-host${ext}`);
const outDir = join(root, "apps", "desktop", "src-tauri", "binaries");
mkdirSync(outDir, { recursive: true });
copyFileSync(src, join(outDir, `velox-host-${target}${ext}`));
// Dev convenience: the app looks for the host next to its own executable.
try {
  const devDir = join(root, "target", release ? "release" : "debug");
  mkdirSync(devDir, { recursive: true });
  copyFileSync(src, join(devDir, `velox-host${ext}`));
} catch {}
console.log(`sidecar ready: velox-host-${target}${ext}`);
