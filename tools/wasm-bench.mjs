// Throughput of the wasm build in Node, one thread: what an engine worker and the
// coordinator worker do in the browser.
//
//   node tools/wasm-bench.mjs [profile.json] [--paths N] [--chunk C] [--repeat R] [--pkg DIR]
//
// (build the package first: `cd web && npm run wasm`). Prints, best of R runs:
// paths/s of `Engine.runChunk` in chunks of C paths; the time to merge those chunks in
// a `Merger`; one `Merger.summarize` (what each progressive picture costs); and the
// size of a chunk's bytes. The last line is one JSON object, for scripts.
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const option = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 ? args.splice(i, 2)[1] : fallback;
};
const paths = Number(option('--paths', '4000'));
const chunk = Number(option('--chunk', '250'));
const repeat = Number(option('--repeat', '5'));
const pkg = resolve(option('--pkg', resolve(root, 'web/src/engine/pkg')));
const file = args[0] ?? resolve(root, 'examples/dual-income-family.json');

const wasm = await import(resolve(pkg, 'firesight_wasm.js'));
wasm.initSync({ module: readFileSync(resolve(pkg, 'firesight_wasm_bg.wasm')) });
const profile = JSON.parse(readFileSync(file, 'utf8'));
const json = JSON.stringify(profile);
const engine = new wasm.Engine(json);
engine.runChunk(0, 20); // warm up

let simulate = Infinity;
let merge = Infinity;
let summarize = Infinity;
let bytes = 0;
let digest = '';
for (let r = 0; r < repeat; r++) {
  const chunks = [];
  let t0 = performance.now();
  for (let first = 0; first < paths; first += chunk) {
    chunks.push(engine.runChunk(first, Math.min(chunk, paths - first)));
  }
  simulate = Math.min(simulate, performance.now() - t0);
  bytes = chunks[0].length;
  const merger = new wasm.Merger();
  t0 = performance.now();
  for (const c of chunks) merger.add(c);
  merge = Math.min(merge, performance.now() - t0);
  t0 = performance.now();
  const summary = merger.summarize(json);
  summarize = Math.min(summarize, performance.now() - t0);
  digest = merger.digest(json);
  merger.free();
  void summary;
}
const result = {
  profile: file.split('/').pop(),
  paths,
  chunk,
  repeat,
  paths_per_sec: Math.round((paths / simulate) * 1000),
  merge_ms: +merge.toFixed(2),
  summarize_ms: +summarize.toFixed(2),
  chunk_bytes: bytes,
  digest_length: digest.length,
};
console.log(`runChunk     ${result.paths_per_sec} paths/s (${paths} paths in chunks of ${chunk}, best of ${repeat})`);
console.log(`merge        ${result.merge_ms} ms for ${Math.ceil(paths / chunk)} chunks`);
console.log(`summarize    ${result.summarize_ms} ms`);
console.log(`chunk bytes  ${bytes}`);
console.log(JSON.stringify(result));
