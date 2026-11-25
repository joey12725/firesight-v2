// An embedded Postgres (PGlite: Postgres compiled to WebAssembly) behind the pool
// interface the server uses. Tests and `npm run dev:api` run on it, so neither needs a
// database installed or a cloud resource. It is a development dependency and is never
// imported by anything under api/.
//
// PGlite is one connection. The pool below hands that connection to one caller at a
// time, so a transaction opened by one request cannot interleave with another's
// statements. That makes it correct, not fast, which is all a dev database needs.

import { PGlite } from '@electric-sql/pglite';
import type { SqlClient, SqlPool } from '../api/_lib/db.js';

export interface EmbeddedPool extends SqlPool {
  /** The underlying database, for tests that want to look at raw rows. */
  readonly pglite: PGlite;
}

const WRITES = new Set(['INSERT', 'UPDATE', 'DELETE', 'MERGE']);

/** `dataDir` persists the database on disk; without it the database lives in memory. */
export async function embeddedPool(dataDir?: string): Promise<EmbeddedPool> {
  const pglite = new PGlite(dataDir);
  await pglite.waitReady;

  let held = false;
  const waiting: Array<() => void> = [];
  const acquire = (): Promise<void> => {
    if (!held) {
      held = true;
      return Promise.resolve();
    }
    return new Promise((resolve) => waiting.push(resolve));
  };
  const release = () => {
    const next = waiting.shift();
    if (next) next();
    else held = false;
  };

  const query = async (text: string | { text: string; values?: unknown[] }, params?: unknown[]) => {
    const sql = typeof text === 'string' ? text : text.text;
    const values = typeof text === 'string' ? params : text.values;
    const result = await pglite.query(sql, values ?? []);
    const command = (result as { command?: string }).command ?? sql.trim().split(/\s+/)[0]?.toUpperCase() ?? '';
    // `pg` reports rows affected for writes and rows returned for reads.
    const rowCount = WRITES.has(command) ? (result.affectedRows ?? result.rows.length) : result.rows.length;
    return { rows: result.rows, rowCount, command };
  };

  // One client object, reused: Kysely keys its connection state on the client's identity.
  const client: SqlClient = { query, release };

  return {
    pglite,
    single: true,
    async connect() {
      await acquire();
      return client;
    },
    async query(text, params) {
      await acquire();
      try {
        return await query(text, params);
      } finally {
        release();
      }
    },
    async end() {
      await pglite.close();
    },
  };
}
