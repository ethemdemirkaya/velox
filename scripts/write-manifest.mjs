// Writes extension/manifest.json. The `key` pins the extension ID so the
// native messaging host can whitelist it. Usage:
//   node scripts/write-manifest.mjs <key.json produced by extension-key.mjs>
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const { key, id } = JSON.parse(readFileSync(process.argv[2], "utf8").replace(/^﻿/, ""));
const version = JSON.parse(readFileSync(join(root, "apps/desktop/package.json"), "utf8")).version;

const manifest = {
  manifest_version: 3,
  name: "Velox — Download Accelerator",
  short_name: "Velox",
  version,
  description: "Send downloads and video streams to Velox, the blazing-fast open-source download manager.",
  key,
  minimum_chrome_version: "116",
  icons: { 16: "icons/icon16.png", 32: "icons/icon32.png", 48: "icons/icon48.png", 128: "icons/icon128.png" },
  action: { default_popup: "popup.html", default_title: "Velox", default_icon: { 16: "icons/icon16.png", 32: "icons/icon32.png" } },
  background: { service_worker: "background.js", type: "module" },
  permissions: ["downloads", "contextMenus", "nativeMessaging", "cookies", "webRequest", "storage", "tabs"],
  host_permissions: ["<all_urls>"],
  content_scripts: [{ matches: ["<all_urls>"], js: ["content.js"], run_at: "document_start", all_frames: true }],
};
writeFileSync(join(root, "extension/manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
console.log(`manifest written – extension id ${id}`);
