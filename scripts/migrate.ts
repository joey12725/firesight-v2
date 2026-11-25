// npm run migrate
//
// Brings the database named by DATABASE_URL_UNPOOLED (or DATABASE_URL) up to date:
// Better Auth's tables, then db/migrations/*.sql. Safe to run on every deploy.
//
//   DATABASE_URL_UNPOOLED=postgres://… npm run migrate
//
// Only the connection string is needed. The schema does not depend on the secrets or on
// the site's origin, so when those are absent stand-ins are used to build the auth
// configuration; nothing is written with them.

import { randomBytes } from 'node:crypto';
import { createPool } from '../api/_lib/db.js';
import { readConfig } from '../api/_lib/env.js';
import { createRuntime } from '../api/_lib/runtime.js';
import { migrate } from '../db/migrate.js';

const env = { ...process.env };
const url = env.DATABASE_URL_UNPOOLED?.trim() || env.DATABASE_URL?.trim();
if (!url) {
  console.error('Set DATABASE_URL_UNPOOLED (or DATABASE_URL) to the database to migrate.');
  process.exit(1);
}
env.DATABASE_URL = url;
env.BETTER_AUTH_SECRET ??= randomBytes(32).toString('base64');
env.PLAN_KEK ??= randomBytes(32).toString('base64');
env.BETTER_AUTH_URL ??= 'http://localhost';

const config = readConfig(env);
if (!config) {
  console.error('The configuration is incomplete.');
  process.exit(1);
}
const pool = createPool(config.migrationUrl);
try {
  const runtime = createRuntime(config, pool);
  const report = await migrate(runtime.db, runtime.auth);
  const host = new URL(config.migrationUrl).host;
  console.log(`Migrated ${host}.`);
  console.log(`  auth tables created: ${report.authTablesCreated.join(', ') || 'none'}`);
  console.log(`  auth tables altered: ${report.authTablesAltered.join(', ') || 'none'}`);
  console.log(`  sql files applied:   ${report.applied.join(', ') || 'none (already up to date)'}`);
} catch (error) {
  console.error(`Migration failed: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
} finally {
  await pool.end();
}
