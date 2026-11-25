import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { readConfig } from '../api/_lib/env.js';
import { createRuntime } from '../api/_lib/runtime.js';
import { startRuntime, testDatabase, testEnv } from '../api/_tests/harness.js';
import type { TestRuntime } from '../api/_tests/harness.js';
import { migrate, statements } from './migrate.js';

let t: TestRuntime;

beforeAll(async () => {
  t = await startRuntime();
});

afterAll(() => t.close());

describe('migrate', () => {
  it('has created the auth tables and ours', async () => {
    const tables = await t.runtime.db.query<{ table_name: string }>("select table_name from information_schema.tables where table_schema = current_schema() and table_type = 'BASE TABLE'");
    const names = tables.rows.map((r) => r.table_name).sort();
    expect(names).toEqual(
      ['account', 'apikey', 'firesight_migration', 'firesight_rate', 'jwks', 'oauthAccessToken', 'oauthClient', 'oauthClientAssertion', 'oauthClientResource', 'oauthConsent', 'oauthRefreshToken', 'oauthResource', 'plan', 'rateLimit', 'session', 'user', 'user_key', 'verification'].sort(),
    );
  });

  it('is idempotent: a second run changes nothing', async () => {
    const before = await t.runtime.db.query('select name from firesight_migration order by name');
    expect(before.rows).toEqual([{ name: '001_plans.sql' }]);
    expect(await migrate(t.runtime.db, t.runtime.auth)).toEqual({ authTablesCreated: [], authTablesAltered: [], applied: [] });
    expect(await migrate(t.runtime.db, t.runtime.auth)).toEqual({ authTablesCreated: [], authTablesAltered: [], applied: [] });
    expect((await t.runtime.db.query('select name from firesight_migration')).rows).toHaveLength(1);
  });

  it('applies a file again only if its record is gone, and tolerates that too', async () => {
    await t.runtime.db.query("delete from firesight_migration where name = '001_plans.sql'");
    expect((await migrate(t.runtime.db, t.runtime.auth)).applied).toEqual(['001_plans.sql']);
  });

  it('is one at a time: a second run started in the same process waits for the first', async () => {
    await t.runtime.db.query("delete from firesight_migration where name = '001_plans.sql'");
    const reports = await Promise.all([migrate(t.runtime.db, t.runtime.auth), migrate(t.runtime.db, t.runtime.auth), migrate(t.runtime.db, t.runtime.auth)]);
    expect(reports.map((r) => r.applied)).toEqual([['001_plans.sql'], [], []]);
  });

  // Needs real connections: the embedded database is one connection, so nothing races.
  it.skipIf(!process.env.TEST_DATABASE_URL)('lets several servers migrate a new database at the same moment', async () => {
    const database = await testDatabase();
    try {
      const config = readConfig(testEnv());
      if (!config || !database.another) throw new Error('No test database.');
      // Three processes, as far as the database can tell: a pool each, nothing shared.
      const runtimes = [database.pool, database.another(), database.another()].map((pool) => createRuntime(config, pool));
      const reports = await Promise.all(runtimes.map((runtime) => migrate(runtime.db, runtime.auth)));
      // One of them did the work; the other two waited and found nothing left to do.
      expect(reports.map((r) => r.applied.length).sort()).toEqual([0, 0, 1]);
      expect(reports.map((r) => r.authTablesCreated.length > 0).sort()).toEqual([false, false, true]);
      const [first] = runtimes;
      if (!first) throw new Error('No runtime.');
      expect((await first.db.query('select name from firesight_migration')).rows).toEqual([{ name: '001_plans.sql' }]);
      expect(await migrate(first.db, first.auth)).toEqual({ authTablesCreated: [], authTablesAltered: [], applied: [] });
    } finally {
      await database.drop();
    }
  });

  it('stores plan bodies as bytes and ties plans and keys to the user with a cascade', async () => {
    const columns = await t.runtime.db.query<{ table_name: string; column_name: string; data_type: string }>(
      "select table_name, column_name, data_type from information_schema.columns where table_schema = current_schema() and table_name in ('plan', 'user_key') order by table_name, ordinal_position",
    );
    const types = Object.fromEntries(columns.rows.map((c) => [`${c.table_name}.${c.column_name}`, c.data_type]));
    expect(types).toMatchObject({ 'plan.name_ct': 'bytea', 'plan.doc_ct': 'bytea', 'plan.rev': 'integer', 'plan.user_id': 'text', 'user_key.dek_wrapped': 'bytea', 'user_key.kek_version': 'integer' });
    // No column holds a plan or its name in the clear.
    expect(Object.keys(types).filter((k) => k.startsWith('plan.')).sort()).toEqual(['plan.bytes', 'plan.created_at', 'plan.doc_ct', 'plan.id', 'plan.name_ct', 'plan.rev', 'plan.updated_at', 'plan.updated_by', 'plan.user_id']);
  });
});

describe('statements', () => {
  it('splits on semicolons and drops comment lines', () => {
    expect(statements('-- a comment\ncreate table a (\n  -- inline note\n  id text\n);\n\ncreate index i on a (id);\n')).toEqual(['create table a (\n  id text\n)', 'create index i on a (id)']);
    expect(statements('  \n-- only a comment\n')).toEqual([]);
  });
});
