// The database seam. Production uses a `pg` pool; tests and `npm run dev:api` use an
// embedded Postgres behind the same two interfaces (db/embedded.ts), so every query in
// this directory runs unchanged against both.

import pg from 'pg';

export interface QueryResult<Row> {
  rows: Row[];
  rowCount: number;
}

export interface Queryable {
  query<Row = Record<string, unknown>>(text: string, params?: readonly unknown[]): Promise<QueryResult<Row>>;
}

export interface Db extends Queryable {
  /** Runs `work` in one transaction: committed when it resolves, rolled back when it throws. */
  transaction<T>(work: (tx: Queryable) => Promise<T>): Promise<T>;
  /**
   * Runs `work` while holding the advisory lock `key`, so that one caller at a time does
   * so across every process connected to the database; the rest wait their turn. `work`
   * uses the database as usual: the lock is held on a connection of its own. It is
   * released when `work` settles, and by the server if this process dies first.
   */
  exclusive<T>(key: number, work: () => Promise<T>): Promise<T>;
}

/** The part of a `pg` client this code and Better Auth's query builder use. */
export interface SqlClient {
  query(text: string, params?: unknown[]): Promise<{ rows: unknown[]; rowCount: number | null; command?: string }>;
  release(): void;
}

/** The part of `pg.Pool` this code and Better Auth's query builder use. */
export interface SqlPool {
  connect(): Promise<SqlClient>;
  query(text: string, params?: unknown[]): Promise<{ rows: unknown[]; rowCount: number | null; command?: string }>;
  end(): Promise<void>;
  /**
   * True for a pool that is one connection handed to one caller at a time (the embedded
   * database). Holding a client from it while running other queries would wait for ever.
   */
  readonly single?: boolean;
}

export function createPool(connectionString: string): SqlPool {
  // A function instance serves one request at a time under most load, and the pooled
  // Neon endpoint multiplexes behind it, so a handful of connections is plenty.
  const pool = new pg.Pool({ connectionString, max: 4, idleTimeoutMillis: 10_000, connectionTimeoutMillis: 8_000 });
  // An idle client that errors (the server closed it) must not crash the process.
  pool.on('error', () => undefined);
  return pool as unknown as SqlPool;
}

export function poolDb(pool: SqlPool): Db {
  const run = async <Row>(target: Pick<SqlPool, 'query'> | SqlClient, text: string, params?: readonly unknown[]): Promise<QueryResult<Row>> => {
    const result = await target.query(text, params ? [...params] : []);
    return { rows: result.rows as Row[], rowCount: result.rowCount ?? result.rows.length };
  };
  // Callers in this process, in order. All that a single-connection pool needs, and it
  // keeps this process to one waiting connection on a real one.
  let queue: Promise<unknown> = Promise.resolve();
  return {
    query: (text, params) => run(pool, text, params),
    exclusive<T>(key: number, work: () => Promise<T>): Promise<T> {
      const locked = async (): Promise<T> => {
        if (pool.single) return work();
        // A transaction-level lock held by an open transaction, not a session-level one:
        // it is tied to one server connection for as long as it is held even behind a
        // transaction-mode pooler, and it cannot outlive the transaction and be left
        // behind on a pooled connection.
        const client = await pool.connect();
        try {
          await client.query('begin');
          await client.query('select pg_advisory_xact_lock($1)', [key]);
          return await work();
        } finally {
          try {
            await client.query('rollback');
          } catch {
            // The connection is gone, and the lock with it.
          }
          client.release();
        }
      };
      const turn = queue.then(locked, locked);
      queue = turn.catch(() => undefined);
      return turn;
    },
    async transaction(work) {
      const client = await pool.connect();
      try {
        await client.query('begin');
        const value = await work({ query: (text, params) => run(client, text, params) });
        await client.query('commit');
        return value;
      } catch (error) {
        try {
          await client.query('rollback');
        } catch {
          // The connection is gone; the server has rolled back already.
        }
        throw error;
      } finally {
        client.release();
      }
    },
  };
}

/** Postgres error code of a failed query, e.g. '23505' for a unique violation. */
export function sqlState(error: unknown): string | undefined {
  if (typeof error !== 'object' || error === null) return undefined;
  const code = (error as { code?: unknown }).code;
  return typeof code === 'string' ? code : undefined;
}
