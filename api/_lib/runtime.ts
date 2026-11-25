// What a function needs to serve a request: the configuration, the database, the auth
// instance and the plan store. Built once per function instance and reused, so a warm
// instance keeps its connection pool and its signing keys.

import { createAuth } from './auth.js';
import type { Auth, AuthLog } from './auth.js';
import { createPool, poolDb } from './db.js';
import type { Db, SqlPool } from './db.js';
import { ConfigError, readConfig } from './env.js';
import type { Env, ServerConfig } from './env.js';
import { createPlanStore } from './plans.js';
import type { PlanStore } from './plans.js';

export interface Runtime {
  readonly config: ServerConfig;
  readonly pool: SqlPool;
  readonly db: Db;
  readonly auth: Auth;
  readonly plans: PlanStore;
}

export function createRuntime(config: ServerConfig, pool: SqlPool, options: { log?: AuthLog } = {}): Runtime {
  const db = poolDb(pool);
  let auth: Auth | undefined;
  return {
    config,
    pool,
    db,
    plans: createPlanStore(db, config),
    // Built on first use: constructing it starts talking to the database, and the
    // endpoints that only describe the deployment (/api/config, the protected-resource
    // document) should answer even when the database is slow to wake.
    get auth() {
      return (auth ??= createAuth(config, { pool, db, log: options.log }));
    },
  };
}

export type RuntimeState = { status: 'ready'; runtime: Runtime } | { status: 'off' } | { status: 'misconfigured'; message: string };

let cached: { env: Env; state: RuntimeState } | null = null;

/**
 * The runtime for this environment: `off` when accounts are not set up, `misconfigured`
 * when they are set up wrongly (the message names the variable, never its value).
 */
export function runtimeFor(env: Env): RuntimeState {
  if (cached?.env === env) return cached.state;
  let state: RuntimeState;
  try {
    const config = readConfig(env);
    state = config ? { status: 'ready', runtime: createRuntime(config, createPool(config.databaseUrl)) } : { status: 'off' };
  } catch (error) {
    if (!(error instanceof ConfigError)) throw error;
    state = { status: 'misconfigured', message: error.message };
  }
  cached = { env, state };
  return state;
}

/** Installs a ready-made runtime for `env`. The dev server and tests use this; functions do not. */
export function provideRuntime(env: Env, runtime: Runtime): void {
  cached = { env, state: { status: 'ready', runtime } };
}
