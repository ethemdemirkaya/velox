// Talks to velox-host exactly like Chrome does (4-byte LE length + JSON),
// so the whole extension → host → app chain can be tested without a browser.
//   node scripts/simulate-extension.mjs <path-to-velox-host> '<json message>'
import { spawn } from "node:child_process";

const [host, json = '{"type":"ping"}'] = process.argv.slice(2);
const child = spawn(host, ["chrome-extension://kgajklbecejimohbelehglchfipdkkke/"], { stdio: ["pipe", "pipe", "inherit"] });
const body = Buffer.from(json);
const len = Buffer.alloc(4);
len.writeUInt32LE(body.length);
child.stdin.write(Buffer.concat([len, body]));

let buf = Buffer.alloc(0);
child.stdout.on("data", (d) => {
  buf = Buffer.concat([buf, d]);
  if (buf.length >= 4 && buf.length >= 4 + buf.readUInt32LE(0)) {
    console.log(buf.subarray(4, 4 + buf.readUInt32LE(0)).toString());
    child.stdin.end();
    child.kill();
  }
});
setTimeout(() => {
  console.log("timeout");
  child.kill();
}, 20000);
