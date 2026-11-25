// Saved households in IndexedDB, one record per household. This module never talks to a
// server: a record leaves the browser only when its owner syncs it to their account
// (state/cloud.ts), and then `remote` says which plan in the account it mirrors.
import { createStore, del, entries, get, set } from 'idb-keyval';
import type { Profile } from '@/engine/types';
import type { RunSettings, Scenario } from './profile-store';

export interface HouseholdRecord {
  id: string;
  name: string;
  origin: string;
  createdAt: number;
  updatedAt: number;
  profile: Profile;
  scenarios: Scenario[];
  run: RunSettings;
  /** Present when this household is synced to the signed-in person's account. */
  remote?: RemoteLink;
}

/** The plan in the account that a local household mirrors. */
export interface RemoteLink {
  /** The plan's id in the account: `pl_…`. */
  id: string;
  /** The account's revision this copy was last level with. Sent back as `If-Match`. */
  rev: number;
  syncedAt: number;
  /** Who wrote that revision: 'web', 'apikey:<name>' or 'oauth:<client>'. */
  updatedBy: string;
  /** The account it belongs to. Another account signed in on this browser leaves it alone. */
  userId: string;
  /** True while this copy has changes the account has not accepted yet (offline, or a conflict). */
  dirty?: boolean;
}

export type HouseholdSummary = Pick<HouseholdRecord, 'id' | 'name' | 'origin' | 'createdAt' | 'updatedAt' | 'remote'>;

// Created lazily: IndexedDB is unavailable in some private modes, and callers handle that.
let store: ReturnType<typeof createStore> | null = null;
const db = () => (store ??= createStore('firesight', 'households'));

export async function listHouseholds(): Promise<HouseholdSummary[]> {
  const all = await entries<string, HouseholdRecord>(db());
  return all
    .map(([, r]) => ({ id: r.id, name: r.name, origin: r.origin, createdAt: r.createdAt, updatedAt: r.updatedAt, remote: r.remote }))
    .sort((a, b) => b.updatedAt - a.updatedAt);
}

export const readHousehold = (id: string) => get<HouseholdRecord>(id, db());
export const writeHousehold = (record: HouseholdRecord) => set(record.id, record, db());
export const deleteHouseholdRecord = (id: string) => del(id, db());

export function newHouseholdId(): string {
  return `h_${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}
