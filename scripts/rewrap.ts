// npm run rewrap
//
// Finishes a rotation of the server key (PLAN_KEK). After a rotation each user's data
// key is rewrapped under the new server key the next time that user's plans are used;
// this does the same for everyone else, so that the old key can be retired.
//
//   npm run rewrap -- --check     only report how many data keys are on each version
//   npm run rewrap                rewrap the rest, then report
//
// It needs the same DATABASE_URL, PLAN_KEK, PLAN_KEK_VERSION and PLAN_KEK_V<old> values
// as the deployment (`vercel env pull`, then export them). It reads and writes the
// `user_key` table only: no plan is decrypted, and nothing is printed but counts. It is
// safe to run while the site is serving, and safe to run again.
//
// Exit status: 0 when every data key is on the current version, 1 otherwise. Remove
// PLAN_KEK_V<old> from the deployment only after a run that exits 0.

import { randomBytes } from 'node:crypto';
import { createPool, poolDb } from '../api/_lib/db.js';
import { ConfigError, readConfig } from '../api/_lib/env.js';
import { keyVersionCounts, rewrapDataKeys } from '../api/_lib/plans.js';

const checkOnly = process.argv.includes('--check');
const env = { ...process.env };
const url = env.DATABASE_URL_UNPOOLED?.trim() || env.DATABASE_URL?.trim();
if (!url) {
  console.error('Set DATABASE_URL (or DATABASE_URL_UNPOOLED) to the database.');
  process.exit(1);
}
if (!env.PLAN_KEK?.trim()) {
  console.error('Set PLAN_KEK, PLAN_KEK_VERSION and PLAN_KEK_V<old version> to the same values as the deployment.');
  process.exit(1);
}
env.DATABASE_URL = url;
// Not used here: only the keys and the database are. Stand-ins let the configuration be read.
env.BETTER_AUTH_SECRET ??= randomBytes(32).toString('base64');
env.BETTER_AUTH_URL ??= 'http://localhost';

let config;
try {
  config = readConfig(env);
} catch (error) {
  if (!(error instanceof ConfigError)) throw error;
  console.error(error.message);
  process.exit(1);
}
if (!config) {
  console.error('The configuration is incomplete.');
  process.exit(1);
}

const line = (counts: Map<number, number>) =>
  [...counts].map(([version, n]) => `v${version}: ${n}${version === config.kekVersion ? ' (current)' : ''}`).join(', ') || 'no data keys yet';

const pool = createPool(url);
try {
  const db = poolDb(pool);
  console.log(`Current server key version: ${config.kekVersion}. Keys held: ${[...config.keks.keys()].sort((a, b) => a - b).map((v) => `v${v}`).join(', ')}.`);
  console.log(`Data keys by version: ${line(await keyVersionCounts(db))}`);
  if (!checkOnly) {
    const report = await rewrapDataKeys(db, config);
    console.log(`Rewrapped ${report.rewrapped}.`);
    for (const [version, n] of report.missingKey) console.log(`  ${n} left on v${version}: PLAN_KEK_V${version} is not set here.`);
    if (report.unreadable > 0) console.log(`  ${report.unreadable} left as they were: the key given for their version does not open them. Check the PLAN_KEK_V<n> values.`);
    console.log(`Data keys by version: ${line(await keyVersionCounts(db))}`);
  }
  const after = await keyVersionCounts(db);
  const behind = [...after].filter(([version]) => version !== config.kekVersion).reduce((sum, [, n]) => sum + n, 0);
  if (behind > 0) {
    console.log(`${behind} data key(s) are not on the current version. Keep every PLAN_KEK_V<n> they need.`);
    process.exitCode = 1;
  } else {
    console.log('Every data key is on the current version. Older PLAN_KEK_V<n> variables are no longer needed; keep a backup of them anyway.');
  }
} catch (error) {
  console.error(`Rewrap failed: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
} finally {
  await pool.end();
}
