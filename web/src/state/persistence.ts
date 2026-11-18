// Saved households in IndexedDB. One record per household; nothing leaves the browser.
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
}

export type HouseholdSummary = Pick<HouseholdRecord, 'id' | 'name' | 'origin' | 'createdAt' | 'updatedAt'>;

// Created lazily: IndexedDB is unavailable in some private modes, and callers handle that.
let store: ReturnType<typeof createStore> | null = null;
const db = () => (store ??= createStore('firesight', 'households'));

export async function listHouseholds(): Promise<HouseholdSummary[]> {
  const all = await entries<string, HouseholdRecord>(db());
  return all
    .map(([, r]) => ({ id: r.id, name: r.name, origin: r.origin, createdAt: r.createdAt, updatedAt: r.updatedAt }))
    .sort((a, b) => b.updatedAt - a.updatedAt);
}

export const readHousehold = (id: string) => get<HouseholdRecord>(id, db());
export const writeHousehold = (record: HouseholdRecord) => set(record.id, record, db());
export const deleteHouseholdRecord = (id: string) => del(id, db());

export function newHouseholdId(): string {
  return `h_${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}
