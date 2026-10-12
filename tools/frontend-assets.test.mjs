import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { gunzipSync, brotliDecompressSync } from "node:zlib";

const root = process.env.LAZYBOY_ASSET_DIR ? resolve(process.env.LAZYBOY_ASSET_DIR) : resolve(dirname(fileURLToPath(import.meta.url)), "../web/dist");
const html = await readFile(resolve(root, "index.html"), "utf8");
const assets = [];
for (const name of await readdir(resolve(root, "assets"))) {
  if (!/\.(js|css)$/.test(name)) continue;
  const path = resolve(root, "assets", name);
  const plain = await readFile(path);
  const entry = html.includes(name);
  if (entry && name.endsWith(".js")) assert(plain.length < 500_000, `Entry JS exceeds 500 KB: ${name}`);
  if (plain.length < 1024) continue;
  const gzip = await readFile(`${path}.gz`);
  const brotli = await readFile(`${path}.br`);
  assert.deepEqual(gunzipSync(gzip), plain, `Gzip contains outdated code: ${name}`);
  assert.deepEqual(brotliDecompressSync(brotli), plain, `Brotli contains outdated code: ${name}`);
  assets.push({ name, bytes: plain.length, gzipBytes: gzip.length, brotliBytes: brotli.length, entry });
}
assert(!html.includes("lottie"));
assert(!(await readdir(resolve(root, "assets"))).some(name => name.endsWith(".woff")), "Unused legacy font copies remain");
assert(assets.some(asset => asset.name.startsWith("markdown-engine-")), "Markdown parser must load separately");
if (process.argv.includes("--json")) console.log(JSON.stringify(assets, null, 2));
else console.log("PASS: frontend size budget, separate Markdown parser, WOFF2 fonts, and exact gzip/Brotli contents.");
