// Test support: a fresh, migrated runtime per test file and a browser stand-in that
// drives the real handlers in process.
//
// The database is the embedded Postgres by default. Set TEST_DATABASE_URL to run the
// same tests through the `pg` driver against a real server (each file then works in a
// schema of its own, dropped afterwards), which is how the production driver path is
// checked.

import { randomBytes } from 'node:crypto';
import pg from 'pg';
import { handle as handleAuth } from '../auth.js';
import { handle as handlePlans } from '../plans.js';
import { handle as handleWellKnown } from '../wellknown.js';
import type { SqlPool } from '../_lib/db.js';
import { readConfig } from '../_lib/env.js';
import type { Env, ServerConfig } from '../_lib/env.js';
import { createRuntime } from '../_lib/runtime.js';
import type { Runtime, RuntimeState } from '../_lib/runtime.js';
import { embeddedPool } from '../../db/embedded.js';
import { migrate } from '../../db/migrate.js';

export const ORIGIN = 'https://firesight.test';
export const PASSWORD = 'correct horse battery staple';

export interface TestRuntime {
  runtime: Runtime;
  state: RuntimeState;
  config: ServerConfig;
  /** Lines the auth library logged, for tests that assert nothing sensitive is written. */
  logs: string[];
  /** See `TestDatabase.another`. */
  another: (() => SqlPool) | null;
  close(): Promise<void>;
}

export function testEnv(overrides: Env = {}): Env {
  return {
    DATABASE_URL: 'postgres://embedded',
    BETTER_AUTH_SECRET: randomBytes(32).toString('base64'),
    PLAN_KEK: randomBytes(32).toString('base64'),
    BETTER_AUTH_URL: ORIGIN,
    ...overrides,
  };
}

/** Connections per pool on a real server, the same as production (`createPool`). */
export const POOL_SIZE = 4;

export interface TestDatabase {
  pool: SqlPool;
  /**
   * Opens a further pool on the same database: connections of its own, as a second
   * server instance would have. Null on the embedded database, which is one connection;
   * tests of real concurrency run only where this exists (TEST_DATABASE_URL).
   */
  another: (() => SqlPool) | null;
  drop(): Promise<void>;
}

/** An empty database (not migrated): embedded, or a schema of its own on TEST_DATABASE_URL. */
export async function testDatabase(): Promise<TestDatabase> {
  const url = process.env.TEST_DATABASE_URL;
  if (!url) {
    const pool = await embeddedPool();
    return { pool, another: null, drop: () => pool.end() };
  }
  const schema = `t_${randomBytes(6).toString('hex')}`;
  const admin = new pg.Client({ connectionString: url });
  await admin.connect();
  await admin.query(`create schema ${schema}`);
  const pools: pg.Pool[] = [];
  const open = () => {
    // Named after the schema, so that a test can find its own sessions in pg_stat_activity.
    const pool = new pg.Pool({ connectionString: url, max: POOL_SIZE, options: `-c search_path=${schema}`, application_name: schema });
    pools.push(pool);
    return pool as unknown as SqlPool;
  };
  return {
    pool: open(),
    another: open,
    async drop() {
      await Promise.all(pools.map((pool) => pool.end()));
      await admin.query(`drop schema ${schema} cascade`);
      await admin.end();
    },
  };
}

/** `rateLimit` is off unless asked for, so tests can sign in as often as they like. */
export async function startRuntime(options: { env?: Env; rateLimit?: boolean } = {}): Promise<TestRuntime> {
  const base = readConfig(testEnv(options.env));
  if (!base) throw new Error('The test configuration is incomplete.');
  const config: ServerConfig = { ...base, rateLimit: options.rateLimit ?? false };
  const { pool, another, drop } = await testDatabase();
  const logs: string[] = [];
  const runtime = createRuntime(config, pool, { log: (level, message) => logs.push(`${level}: ${message}`) });
  await migrate(runtime.db, runtime.auth);
  return { runtime, state: { status: 'ready', runtime }, config, logs, another, close: drop };
}

export interface CallOptions {
  body?: unknown;
  headers?: Record<string, string>;
  /** Send no Origin header, or a different one. By default writes carry the site's own. */
  origin?: string | null;
  /** Send no cookies on this call. */
  anonymous?: boolean;
}

export interface Reply<T = unknown> {
  status: number;
  headers: Headers;
  body: T;
  text: string;
}

let nextAddress = 1;

/**
 * Behaves like one browser profile on the site: keeps cookies, sends `Origin` on
 * writes, and reaches the functions by their public paths.
 */
export class Browser {
  readonly cookies = new Map<string, string>();
  /** A distinct client address per browser, as the platform would report it. */
  readonly address = `203.0.113.${nextAddress++ % 250}`;

  constructor(
    private readonly state: RuntimeState,
    private readonly origin: string = ORIGIN,
  ) {}

  async call<T = unknown>(method: string, path: string, options: CallOptions = {}): Promise<Reply<T>> {
    const headers = new Headers({ 'x-forwarded-for': this.address, ...options.headers });
    const write = method !== 'GET' && method !== 'HEAD';
    const origin = options.origin === undefined ? (write ? this.origin : null) : options.origin;
    if (origin) headers.set('origin', origin);
    if (!options.anonymous && this.cookies.size > 0) headers.set('cookie', [...this.cookies].map(([k, v]) => `${k}=${v}`).join('; '));
    let body: string | undefined;
    if (options.body !== undefined) {
      body = typeof options.body === 'string' ? options.body : JSON.stringify(options.body);
      if (!headers.has('content-type')) headers.set('content-type', 'application/json');
    }
    const request = new Request(new URL(path, this.origin), { method, headers, body, redirect: 'manual' });
    const response = await dispatch(request, this.state);
    for (const cookie of response.headers.getSetCookie()) {
      const [pair = '', ...attributes] = cookie.split(';');
      const eq = pair.indexOf('=');
      const name = pair.slice(0, eq).trim();
      const expired = attributes.some((a) => /^\s*max-age=0\s*$/i.test(a));
      if (expired || pair.slice(eq + 1) === '') this.cookies.delete(name);
      else this.cookies.set(name, pair.slice(eq + 1));
    }
    const text = await response.text();
    let parsed: unknown = text;
    try {
      parsed = text ? JSON.parse(text) : null;
    } catch {
      // Not JSON: leave the text.
    }
    return { status: response.status, headers: response.headers, body: parsed as T, text };
  }

  get = <T = unknown>(path: string, options?: CallOptions) => this.call<T>('GET', path, options);
  post = <T = unknown>(path: string, body?: unknown, options?: CallOptions) => this.call<T>('POST', path, { ...options, body });
  put = <T = unknown>(path: string, body?: unknown, options?: CallOptions) => this.call<T>('PUT', path, { ...options, body });
  delete = <T = unknown>(path: string, options?: CallOptions) => this.call<T>('DELETE', path, options);

  /** Creates an account and leaves this browser signed in to it. Returns the user id. */
  async signUp(email: string, name = 'Test Person'): Promise<string> {
    const reply = await this.post<{ user: { id: string } }>('/api/auth/sign-up/email', { email, password: PASSWORD, name });
    if (reply.status !== 200) throw new Error(`Sign-up failed: ${reply.status} ${reply.text}`);
    return reply.body.user.id;
  }
}

/** Routes a request the way the deployment does, with the original path intact. */
export function dispatch(request: Request, state: RuntimeState): Promise<Response> {
  const { pathname } = new URL(request.url);
  const deps = {
    state,
    report: (error: unknown) => {
      throw error;
    },
  };
  if (pathname === '/api/config' || pathname.startsWith('/.well-known/')) return handleWellKnown(request, {}, deps);
  if (pathname === '/api/plans' || pathname.startsWith('/api/plans/')) return handlePlans(request, {}, deps);
  if (pathname.startsWith('/api/auth/')) return handleAuth(request, {}, deps);
  return Promise.resolve(new Response('not routed', { status: 404 }));
}

/** A small but realistic plan in the website's file format. */
export function samplePlan(name = 'Baseline', salary = 120_000) {
  return {
    firesight: 2 as const,
    profile: {
      schema_version: 2,
      name,
      household: { people: [{ name: 'Ada', birth_year: 1988 }] },
      income: { jobs: [{ person: 0, salary, label: 'Engineer at Initech' }] },
      accounts: [{ kind: 'brokerage', balance: 251_337.42 }],
    },
    scenarios: [{ id: 'retire-early', name: 'Retire early', ops: [{ op: 'set', path: '/retirement/age', value: 55 }] }],
    run: { paths: 2000, seed: 7 },
  };
}
