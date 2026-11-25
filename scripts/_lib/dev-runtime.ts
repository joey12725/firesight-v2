// The local runtime behind `npm run dev:api` and the end-to-end scripts: an embedded
// Postgres and generated secrets when the environment names no database, so the whole
// account feature runs on a laptop or in CI without any cloud resource.

import { randomBytes } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { AuthLog } from '../../api/_lib/auth.js';
import { createPool } from '../../api/_lib/db.js';
import type { SqlPool } from '../../api/_lib/db.js';
import { readConfig } from '../../api/_lib/env.js';
import type { Env, ServerConfig } from '../../api/_lib/env.js';
import { createRuntime } from '../../api/_lib/runtime.js';
import type { Runtime } from '../../api/_lib/runtime.js';
import { embeddedPool } from '../../db/embedded.js';
import { migrate } from '../../db/migrate.js';
import type { MigrationReport } from '../../db/migrate.js';

export const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
export const LOCAL_DIR = join(REPO_ROOT, '.local');
/** The web app's dev server: the origin a person actually browses during development. */
export const DEFAULT_DEV_ORIGIN = 'http://localhost:5173';

export interface DevRuntime {
  runtime: Runtime;
  migration: MigrationReport;
  /** 'postgres' when DATABASE_URL was set, otherwise where the embedded database lives. */
  database: 'postgres' | 'embedded (in memory)' | `embedded (${string})`;
  close(): Promise<void>;
}

interface DevSecrets {
  BETTER_AUTH_SECRET: string;
  PLAN_KEK: string;
}

/** Dev-only secrets, generated once and kept beside the dev database they belong to. */
function devSecrets(dir: string | null): DevSecrets {
  const fresh = (): DevSecrets => ({ BETTER_AUTH_SECRET: randomBytes(32).toString('base64'), PLAN_KEK: randomBytes(32).toString('base64') });
  if (!dir) return fresh();
  const file = join(dir, 'dev-secrets.json');
  if (existsSync(file)) {
    const saved = JSON.parse(readFileSync(file, 'utf8')) as Partial<DevSecrets>;
    if (typeof saved.BETTER_AUTH_SECRET === 'string' && typeof saved.PLAN_KEK === 'string') return { BETTER_AUTH_SECRET: saved.BETTER_AUTH_SECRET, PLAN_KEK: saved.PLAN_KEK };
  }
  const secrets = fresh();
  mkdirSync(dir, { recursive: true });
  writeFileSync(file, `${JSON.stringify(secrets, null, 2)}\n`, { mode: 0o600 });
  return secrets;
}

const alive = (pid: number): boolean => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

/** The embedded database is one process's to open. A second opener would corrupt it. */
function lock(dir: string): () => void {
  const file = join(dir, 'dev-api.lock');
  if (existsSync(file)) {
    const pid = Number(readFileSync(file, 'utf8').trim());
    if (Number.isInteger(pid) && pid !== process.pid && alive(pid)) {
      throw new Error(`The dev database in ${dir} is in use by process ${pid}. Stop that server, or start this one with FIRESIGHT_DEV_DATA=memory (or another directory).`);
    }
  }
  mkdirSync(dir, { recursive: true });
  writeFileSync(file, String(process.pid));
  return () => rmSync(file, { force: true });
}

/**
 * Builds and migrates the runtime.
 *
 * With `DATABASE_URL` set, the environment is used as it is (a real Postgres, real
 * secrets). Without it: an embedded database under `.local/` (or `FIRESIGHT_DEV_DATA`,
 * where `memory` means nothing is kept), secrets generated into the same directory, and
 * `BETTER_AUTH_URL` defaulting to the web dev server's origin.
 */
export async function startDevRuntime(env: Env, options: { log?: AuthLog } = {}): Promise<DevRuntime> {
  let config: ServerConfig | null;
  let pool: SqlPool;
  let database: DevRuntime['database'];
  let unlock = () => {};

  if (env.DATABASE_URL?.trim()) {
    config = readConfig(env);
    if (!config) throw new Error('DATABASE_URL is set, so BETTER_AUTH_SECRET, PLAN_KEK and BETTER_AUTH_URL must be set too (see .env.example).');
    pool = createPool(config.migrationUrl);
    database = 'postgres';
  } else {
    const where = env.FIRESIGHT_DEV_DATA?.trim();
    const dir = where === 'memory' ? null : where ? where : LOCAL_DIR;
    if (dir) unlock = lock(dir);
    const secrets = devSecrets(dir);
    config = readConfig({
      ...env,
      DATABASE_URL: 'embedded',
      BETTER_AUTH_SECRET: env.BETTER_AUTH_SECRET ?? secrets.BETTER_AUTH_SECRET,
      PLAN_KEK: env.PLAN_KEK ?? secrets.PLAN_KEK,
      BETTER_AUTH_URL: env.BETTER_AUTH_URL ?? DEFAULT_DEV_ORIGIN,
    });
    if (!config) throw new Error('The dev configuration could not be built.');
    try {
      pool = await embeddedPool(dir ? join(dir, 'pgdata') : undefined);
    } catch (error) {
      unlock();
      throw error;
    }
    database = dir ? `embedded (${dir})` : 'embedded (in memory)';
  }

  const runtime = createRuntime(config, pool, options);
  const migration = await migrate(runtime.db, runtime.auth);
  return {
    runtime,
    migration,
    database,
    async close() {
      await pool.end();
      unlock();
    },
  };
}
