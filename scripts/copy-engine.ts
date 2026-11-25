// npm run engine
//
// Copies the simulation engine (the wasm module and its JavaScript glue) from the web
// app's build output, web/src/engine/pkg/, to api/_engine/, where the MCP function
// loads it from. Both directories are build artefacts and neither is in git.
//
// The copy exists so that the function's imports stay inside api/: the platform's
// bundler then needs nothing from the web app's source tree, and the engine a function
// was built with is plainly the one sitting next to it.
//
//   --if-present   exit quietly when the engine has not been built but a copy from an
//                  earlier build is already in place (the npm "pre" hooks pass this)
//   --optional     as --if-present, and when there is no copy either, warn instead of
//                  failing (the dev server: everything but the MCP endpoint still works)

import { copyFileSync, existsSync, mkdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { REPO_ROOT } from './_lib/dev-runtime.js';

const SOURCE = join(REPO_ROOT, 'web', 'src', 'engine', 'pkg');
const TARGET = join(REPO_ROOT, 'api', '_engine');
const FILES = ['firesight_wasm.js', 'firesight_wasm.d.ts', 'firesight_wasm_bg.wasm'];
const optional = process.argv.includes('--optional');
const ifPresent = optional || process.argv.includes('--if-present');

const missing = FILES.filter((file) => !existsSync(join(SOURCE, file)));
if (missing.length > 0) {
  const haveCopy = FILES.every((file) => existsSync(join(TARGET, file)));
  if (ifPresent && haveCopy) process.exit(0);
  console.error(
    `${optional ? 'The MCP endpoint will not work yet. ' : ''}The engine has not been built: ${missing.map((file) => `web/src/engine/pkg/${file}`).join(', ')} ${missing.length === 1 ? 'is' : 'are'} missing.\n` +
      'Build it with `npm install --prefix web && npm run wasm --prefix web` (needs Rust with the wasm32-unknown-unknown target), then run this again.',
  );
  process.exit(optional ? 0 : 1);
}

mkdirSync(TARGET, { recursive: true });
let copied = 0;
for (const file of FILES) {
  const from = join(SOURCE, file);
  const to = join(TARGET, file);
  // Unchanged files are left alone, so a watcher or a running dev server sees no churn.
  if (existsSync(to) && statSync(to).size === statSync(from).size && readFileSync(to).equals(readFileSync(from))) continue;
  copyFileSync(from, to);
  copied++;
}
if (!ifPresent || copied > 0) console.log(`api/_engine: ${copied === 0 ? 'up to date' : `${copied} file${copied === 1 ? '' : 's'} copied from web/src/engine/pkg`}`);
