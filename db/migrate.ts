// Brings a database up to date: Better Auth's schema first (its tables are referenced
// by ours), then the numbered SQL files in db/migrations. Safe to run again and again;
// each SQL file is applied once, in a transaction, and recorded.

import { readdir, readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { getMigrations } from 'better-auth/db/migration';
import type { Auth } from '../api/_lib/auth.js';
import type { Db } from '../api/_lib/db.js';

const MIGRATIONS_DIR = join(dirname(fileURLToPath(import.meta.url)), 'migrations');

export interface MigrationReport {
  /** Better Auth tables created and tables that gained columns. */
  authTablesCreated: string[];
  authTablesAltered: string[];
  /** SQL files applied in this run. */
  applied: string[];
}

/** The advisory lock a migration holds from start to finish. */
const MIGRATION_LOCK = 4117202601;

export function migrate(db: Db, auth: Auth): Promise<MigrationReport> {
  // One migration at a time, across every process pointed at this database. Better
  // Auth's `create table` statements are not idempotent, so two runs that both found a
  // new database would otherwise collide. A run that had to wait works out what is
  // missing only once it holds the lock, and so finds the first run's tables in place.
  return db.exclusive(MIGRATION_LOCK, async () => {
    const plan = await getMigrations(auth.options);
    const authTablesCreated = plan.toBeCreated.map((t) => t.table);
    const authTablesAltered = plan.toBeAdded.map((t) => t.table);
    await plan.runMigrations();

    await db.query('create table if not exists firesight_migration (name text primary key, applied_at timestamptz not null default now())');
    const files = (await readdir(MIGRATIONS_DIR)).filter((f) => f.endsWith('.sql')).sort();
    const applied: string[] = [];
    for (const file of files) {
      const sql = await readFile(join(MIGRATIONS_DIR, file), 'utf8');
      const ran = await db.transaction(async (tx) => {
        const seen = await tx.query('select 1 from firesight_migration where name = $1', [file]);
        if (seen.rows.length > 0) return false;
        for (const statement of statements(sql)) await tx.query(statement);
        await tx.query('insert into firesight_migration (name) values ($1)', [file]);
        return true;
      });
      if (ran) applied.push(file);
    }
    return { authTablesCreated, authTablesAltered, applied };
  });
}

/**
 * Splits a migration file into statements. The extended query protocol takes one
 * statement at a time. The files here are plain DDL: no semicolons inside strings or
 * function bodies, which this does not try to handle.
 */
export function statements(sql: string): string[] {
  return sql
    .split('\n')
    .filter((line) => !line.trim().startsWith('--'))
    .join('\n')
    .split(';')
    .map((s) => s.trim())
    .filter(Boolean);
}
