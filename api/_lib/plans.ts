// Synced plans: the only module that reads or writes the `plan` and `user_key` tables.
// The website's REST API (api/plans.ts) and the MCP server share it.
//
// There is no row-level security in the database, so this module is the guard. Every
// statement that touches a plan names the owner as well as the id, and the owner always
// comes from a verified credential, never from a request body or a tool argument. A
// plan that belongs to someone else is indistinguishable from one that does not exist.

import { randomBytes } from 'node:crypto';
import { z } from 'zod';
import { DecryptError, newDataKey, open, planAad, seal, unwrapDataKey, wrapDataKey } from './crypto.js';
import type { Db, Queryable } from './db.js';
import { sqlState } from './db.js';
import type { ServerConfig } from './env.js';

export const MAX_PLANS_PER_USER = 50;
/** Largest plan, as UTF-8 JSON. A full household with scenarios is a few tens of kilobytes. */
export const MAX_PLAN_BYTES = 256 * 1024;
export const MAX_NAME_LENGTH = 120;
export const DEFAULT_PLAN_NAME = 'Untitled plan';

/**
 * A plan as the website saves it (`PlanFile` in web/src/state/files.ts). Only the
 * envelope is checked here. The profile inside is checked by the engine: in the browser
 * before it is sent, and in the MCP server before anything is stored or simulated.
 */
export interface PlanFile {
  firesight: 2;
  profile: Record<string, unknown>;
  scenarios: unknown[];
  run?: Record<string, unknown>;
}

const planFileSchema = z.strictObject({
  firesight: z.literal(2),
  profile: z.record(z.string(), z.unknown()),
  scenarios: z.array(z.unknown()).max(200),
  run: z.record(z.string(), z.unknown()).optional(),
});

/** Who made a write. Shown to the user ("updated by Claude Code key"), so it is cleaned text. */
export type PlanWriter = 'web' | `apikey:${string}` | `oauth:${string}`;

export interface PlanMeta {
  id: string;
  name: string;
  rev: number;
  /** ISO 8601, UTC. */
  created_at: string;
  updated_at: string;
  updated_by: string;
  /** Size of the plan as UTF-8 JSON. */
  bytes: number;
}

export interface StoredPlan extends PlanMeta {
  plan: PlanFile;
}

export type PlanErrorCode = 'invalid_plan' | 'too_large' | 'too_many_plans' | 'not_found' | 'conflict';

export class PlanError extends Error {
  constructor(
    readonly code: PlanErrorCode,
    message: string,
    /** For `conflict`: the plan as it stands now, so the caller can re-read and retry. */
    readonly current?: PlanMeta,
  ) {
    super(message);
    this.name = 'PlanError';
  }
}

const BASE62 = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';
const PLAN_ID = /^pl_[0-9A-Za-z]{22}$/;

/** `pl_` and 22 base62 characters: about 131 bits from the system's random source. */
export function newPlanId(): string {
  let id = 'pl_';
  while (id.length < 25) {
    // 248 is the largest multiple of 62 under 256; rejecting the rest keeps every character equally likely.
    for (const byte of randomBytes(32)) {
      if (byte < 248 && id.length < 25) id += BASE62[byte % 62];
    }
  }
  return id;
}

export const isPlanId = (value: unknown): value is string => typeof value === 'string' && PLAN_ID.test(value);

// Control characters, and the invisible and direction-changing characters that can make
// a name render as something other than what was stored (or hide text from a reader).
// eslint-disable-next-line no-control-regex -- matching control characters is the point
const UNSAFE_TEXT = /[\u0000-\u001f\u007f-\u009f\u00ad\u061c\u200b-\u200f\u2028-\u202e\u2060-\u2069\ufeff]/g;

/** Plain single-line text of bounded length, or '' when nothing printable is left. */
export function cleanText(value: unknown, maxLength: number): string {
  if (typeof value !== 'string') return '';
  const text = value.normalize('NFC').replace(UNSAFE_TEXT, ' ').replace(/\s+/g, ' ').trim();
  // Cut on code points so a surrogate pair is never split.
  return Array.from(text).slice(0, maxLength).join('').trim();
}

/** The `updated_by` value for a write made with an API key or by an OAuth client. */
export function writerLabel(kind: 'apikey' | 'oauth', name: unknown): PlanWriter {
  return `${kind}:${cleanText(name, 64) || 'unnamed'}`;
}

function cleanWriter(by: PlanWriter): string {
  if (by === 'web') return by;
  const colon = by.indexOf(':');
  const kind = by.slice(0, colon);
  if (kind !== 'apikey' && kind !== 'oauth') throw new TypeError('Unknown plan writer.');
  return writerLabel(kind, by.slice(colon + 1));
}

function planName(plan: PlanFile, name: string | undefined): string {
  return cleanText(name, MAX_NAME_LENGTH) || cleanText(plan.profile.name, MAX_NAME_LENGTH) || DEFAULT_PLAN_NAME;
}

/** Checks the envelope and the size, and returns the canonical JSON bytes to store. */
function serialise(plan: unknown): { plan: PlanFile; json: Buffer } {
  const parsed = planFileSchema.safeParse(plan);
  if (!parsed.success) {
    const issue = parsed.error.issues[0];
    const where = issue && issue.path.length > 0 ? ` (at ${issue.path.join('.')})` : '';
    throw new PlanError('invalid_plan', `The plan must be { firesight: 2, profile: {…}, scenarios: […], run?: {…} }${where}.`);
  }
  const json = Buffer.from(JSON.stringify(parsed.data), 'utf8');
  if (json.length > MAX_PLAN_BYTES) throw new PlanError('too_large', `The plan is ${json.length} bytes; the limit is ${MAX_PLAN_BYTES}.`);
  return { plan: parsed.data as PlanFile, json };
}

interface PlanRow {
  id: string;
  name_ct: Uint8Array;
  doc_ct?: Uint8Array;
  bytes: number;
  rev: number;
  updated_by: string;
  created_at: Date;
  updated_at: Date;
}

const META_COLUMNS = 'id, name_ct, bytes, rev, updated_by, created_at, updated_at';

export interface PlanStore {
  /** Newest first. Never includes plan bodies. */
  listPlans(userId: string): Promise<PlanMeta[]>;
  /** Null when the plan does not exist or belongs to someone else. */
  getPlan(userId: string, id: string): Promise<StoredPlan | null>;
  /** Throws PlanError: invalid_plan, too_large, too_many_plans. */
  createPlan(userId: string, plan: unknown, by: PlanWriter, name?: string): Promise<PlanMeta>;
  /**
   * Replaces the plan if it is still at `expectedRev`. Without `name` the name is kept.
   * Throws PlanError: invalid_plan, too_large, not_found, conflict (with `current`).
   */
  updatePlan(userId: string, id: string, plan: unknown, expectedRev: number, by: PlanWriter, name?: string): Promise<PlanMeta>;
  /** False when there was nothing to delete. */
  deletePlan(userId: string, id: string): Promise<boolean>;
  /** Every plan with its body, newest first: the user's data export. */
  exportPlans(userId: string): Promise<StoredPlan[]>;
}

export function createPlanStore(db: Db, keys: Pick<ServerConfig, 'keks' | 'kekVersion'>): PlanStore {
  const kek = (version: number): Buffer => {
    const key = keys.keks.get(version);
    // A data key wrapped by a key the server no longer has is unreadable, like any other failed decrypt.
    if (!key) throw new DecryptError();
    return key;
  };

  /**
   * The user's data key, created on first use. `lock` takes a row lock that lasts until
   * the transaction ends, which serialises that user's plan creations so the plan limit
   * cannot be raced past.
   */
  async function dataKey(tx: Queryable, userId: string, options: { create: boolean; lock?: boolean }): Promise<Buffer | null> {
    const select = `select dek_wrapped, kek_version from user_key where user_id = $1${options.lock ? ' for update' : ''}`;
    let found = await tx.query<{ dek_wrapped: Uint8Array; kek_version: number }>(select, [userId]);
    if (found.rows.length === 0) {
      if (!options.create) return null;
      const fresh = newDataKey();
      // Two first writes at once: one insert wins, both then read the winner's key.
      await tx.query('insert into user_key (user_id, dek_wrapped, kek_version) values ($1, $2, $3) on conflict (user_id) do nothing', [
        userId,
        wrapDataKey(kek(keys.kekVersion), fresh, userId),
        keys.kekVersion,
      ]);
      found = await tx.query(select, [userId]);
    }
    const row = found.rows[0];
    if (!row) throw new DecryptError();
    const key = unwrapDataKey(kek(row.kek_version), row.dek_wrapped, userId);
    if (row.kek_version !== keys.kekVersion) {
      // Key rotation: rewrap under the current server key the next time the user's key is used.
      // Users who do not come back are rewrapped by `rewrapDataKeys` (npm run rewrap).
      await tx.query('update user_key set dek_wrapped = $2, kek_version = $3 where user_id = $1 and kek_version = $4', [
        userId,
        wrapDataKey(kek(keys.kekVersion), key, userId),
        keys.kekVersion,
        row.kek_version,
      ]);
    }
    return key;
  }

  const toMeta = (key: Buffer, userId: string, row: PlanRow): PlanMeta => ({
    id: row.id,
    name: open(key, row.name_ct, planAad(userId, row.id, 'name')).toString('utf8'),
    rev: row.rev,
    created_at: new Date(row.created_at).toISOString(),
    updated_at: new Date(row.updated_at).toISOString(),
    updated_by: row.updated_by,
    bytes: row.bytes,
  });

  const toStored = (key: Buffer, userId: string, row: PlanRow): StoredPlan => {
    if (!row.doc_ct) throw new DecryptError();
    const json = open(key, row.doc_ct, planAad(userId, row.id, 'doc')).toString('utf8');
    return { ...toMeta(key, userId, row), plan: JSON.parse(json) as PlanFile };
  };

  return {
    async listPlans(userId) {
      const key = await dataKey(db, userId, { create: false });
      if (!key) return [];
      const found = await db.query<PlanRow>(`select ${META_COLUMNS} from plan where user_id = $1 order by updated_at desc, id`, [userId]);
      return found.rows.map((row) => toMeta(key, userId, row));
    },

    async getPlan(userId, id) {
      if (!isPlanId(id)) return null;
      const found = await db.query<PlanRow>(`select ${META_COLUMNS}, doc_ct from plan where id = $1 and user_id = $2`, [id, userId]);
      const row = found.rows[0];
      if (!row) return null;
      const key = await dataKey(db, userId, { create: false });
      if (!key) throw new DecryptError();
      return toStored(key, userId, row);
    },

    async createPlan(userId, plan, by, name) {
      const { plan: checked, json } = serialise(plan);
      const writer = cleanWriter(by);
      const title = planName(checked, name);
      // A collision in 131 bits will not happen; the retry is there so that it cannot matter.
      for (let attempt = 0; ; attempt++) {
        const id = newPlanId();
        try {
          return await db.transaction(async (tx) => {
            const key = await dataKey(tx, userId, { create: true, lock: true });
            if (!key) throw new DecryptError();
            const count = await tx.query<{ n: number }>('select count(*)::int as n from plan where user_id = $1', [userId]);
            if ((count.rows[0]?.n ?? 0) >= MAX_PLANS_PER_USER) {
              throw new PlanError('too_many_plans', `An account can hold ${MAX_PLANS_PER_USER} plans. Delete one to make room.`);
            }
            const inserted = await tx.query<PlanRow>(
              `insert into plan (id, user_id, name_ct, doc_ct, bytes, rev, updated_by) values ($1, $2, $3, $4, $5, 1, $6) returning ${META_COLUMNS}`,
              [id, userId, seal(key, Buffer.from(title, 'utf8'), planAad(userId, id, 'name')), seal(key, json, planAad(userId, id, 'doc')), json.length, writer],
            );
            const row = inserted.rows[0];
            if (!row) throw new Error('The plan was not stored.');
            return toMeta(key, userId, row);
          });
        } catch (error) {
          if (sqlState(error) === '23505' && attempt < 2) continue;
          throw error;
        }
      }
    },

    async updatePlan(userId, id, plan, expectedRev, by, name) {
      const { plan: checked, json } = serialise(plan);
      const writer = cleanWriter(by);
      if (!isPlanId(id)) throw new PlanError('not_found', 'There is no such plan.');
      if (!Number.isInteger(expectedRev) || expectedRev < 1) throw new PlanError('conflict', 'The expected rev must be a positive integer.');
      return db.transaction(async (tx) => {
        const key = await dataKey(tx, userId, { create: false });
        if (!key) throw new PlanError('not_found', 'There is no such plan.');
        // An explicit name wins; otherwise the stored name is kept (not re-derived from the profile).
        const title = name === undefined ? null : planName(checked, name);
        const updated = await tx.query<PlanRow>(
          `update plan set doc_ct = $4, bytes = $5, updated_by = $6, name_ct = coalesce($7, name_ct), rev = rev + 1, updated_at = now()
           where id = $1 and user_id = $2 and rev = $3 returning ${META_COLUMNS}`,
          [id, userId, expectedRev, seal(key, json, planAad(userId, id, 'doc')), json.length, writer, title === null ? null : seal(key, Buffer.from(title, 'utf8'), planAad(userId, id, 'name'))],
        );
        const row = updated.rows[0];
        if (row) return toMeta(key, userId, row);
        const current = await tx.query<PlanRow>(`select ${META_COLUMNS} from plan where id = $1 and user_id = $2`, [id, userId]);
        const existing = current.rows[0];
        if (!existing) throw new PlanError('not_found', 'There is no such plan.');
        const meta = toMeta(key, userId, existing);
        throw new PlanError('conflict', `The plan has changed since rev ${expectedRev}: it is now at rev ${meta.rev}. Read it again and reapply the change.`, meta);
      });
    },

    async deletePlan(userId, id) {
      if (!isPlanId(id)) return false;
      const deleted = await db.query('delete from plan where id = $1 and user_id = $2', [id, userId]);
      return deleted.rowCount > 0;
    },

    async exportPlans(userId) {
      const key = await dataKey(db, userId, { create: false });
      if (!key) return [];
      const found = await db.query<PlanRow>(`select ${META_COLUMNS}, doc_ct from plan where user_id = $1 order by updated_at desc, id`, [userId]);
      return found.rows.map((row) => toStored(key, userId, row));
    },
  };
}

export interface RewrapReport {
  /** Data keys moved to the current server key in this run. */
  rewrapped: number;
  /** Data keys left on an older version because this server was not given that version's key: version → count. */
  missingKey: Map<number, number>;
  /** Data keys that the key held for their version does not open (a wrong PLAN_KEK_V<n>, or a damaged row). */
  unreadable: number;
}

/** How many users' data keys are wrapped by each server-key version: version → count. */
export async function keyVersionCounts(db: Db): Promise<Map<number, number>> {
  const found = await db.query<{ kek_version: number; n: number }>('select kek_version, count(*)::int as n from user_key group by kek_version order by kek_version');
  return new Map(found.rows.map((row) => [row.kek_version, row.n]));
}

/**
 * Finishes a server-key rotation: rewraps every data key that is still under an older
 * key, including those of users who have not been back since (`npm run rewrap`). A row
 * that cannot be rewrapped is left exactly as it was and counted. Safe to run while the
 * site is serving and safe to run again.
 */
export async function rewrapDataKeys(db: Db, keys: Pick<ServerConfig, 'keks' | 'kekVersion'>, batch = 200): Promise<RewrapReport> {
  const current = keys.keks.get(keys.kekVersion);
  if (!current) throw new TypeError('There is no key for the current version.');
  const report: RewrapReport = { rewrapped: 0, missingKey: new Map(), unreadable: 0 };
  for (let after = ''; ; ) {
    const page = await db.query<{ user_id: string; dek_wrapped: Uint8Array; kek_version: number }>(
      'select user_id, dek_wrapped, kek_version from user_key where kek_version <> $1 and user_id > $2 order by user_id limit $3',
      [keys.kekVersion, after, batch],
    );
    for (const row of page.rows) {
      after = row.user_id;
      const old = keys.keks.get(row.kek_version);
      if (!old) {
        report.missingKey.set(row.kek_version, (report.missingKey.get(row.kek_version) ?? 0) + 1);
        continue;
      }
      let key: Buffer;
      try {
        key = unwrapDataKey(old, row.dek_wrapped, row.user_id);
      } catch (error) {
        if (!(error instanceof DecryptError)) throw error;
        report.unreadable += 1;
        continue;
      }
      // The same guarded update a request makes: if one got there first, this changes nothing.
      const updated = await db.query('update user_key set dek_wrapped = $2, kek_version = $3 where user_id = $1 and kek_version = $4', [
        row.user_id,
        wrapDataKey(current, key, row.user_id),
        keys.kekVersion,
        row.kek_version,
      ]);
      report.rewrapped += updated.rowCount;
    }
    if (page.rows.length < batch) return report;
  }
}

