// Generates (once) the RSA key that pins the unpacked extension's ID and
// prints {"key": <manifest key>, "id": <extension id>}.
// Usage: node scripts/extension-key.mjs <path-to-private-key.pem>
import { createHash, createPrivateKey, createPublicKey, generateKeyPairSync } from "node:crypto";
import { existsSync, writeFileSync, readFileSync } from "node:fs";

const file = process.argv[2] ?? "extension-key.pem";
let priv;
if (existsSync(file)) {
  priv = createPrivateKey(readFileSync(file));
} else {
  priv = generateKeyPairSync("rsa", { modulusLength: 2048 }).privateKey;
  writeFileSync(file, priv.export({ type: "pkcs8", format: "pem" }));
}
const der = createPublicKey(priv).export({ type: "spki", format: "der" });
const id = [...createHash("sha256").update(der).digest("hex").slice(0, 32)]
  .map((h) => String.fromCharCode(97 + parseInt(h, 16)))
  .join("");
console.log(JSON.stringify({ key: der.toString("base64"), id }));
