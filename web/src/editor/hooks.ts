import { useCallback, useMemo } from 'react';
import type { Issue, PatchOp, Profile, Report } from '@/engine/types';
import { canonicalPath, getAt, isWithin } from '@/state/pointer';
import { selectEffectiveProfile, selectEffectiveReport, useProfileStore, type EditOptions } from '@/state/profile-store';
import { toast } from '@/state/ui-store';
import type { FieldIssue } from '@/ui';
import { describe } from './schema';

/** The profile the editor is showing (baseline, or baseline + the scenario being edited). */
export function useProfile(): Profile {
  const profile = useProfileStore(selectEffectiveProfile);
  if (!profile) throw new Error('The editor is mounted without an open plan.');
  return profile;
}

/** The value at a patch path. Re-renders when it changes. */
export function useValue<T>(path: string): T {
  return useProfileStore((s) => getAt(selectEffectiveProfile(s), path)) as T;
}

/** Applies patch operations to the current edit target; reports failures as a toast. */
export function useEdit() {
  return useCallback((ops: PatchOp | PatchOp[], options?: EditOptions) => {
    const result = useProfileStore.getState().edit(Array.isArray(ops) ? ops : [ops], options);
    if (!result.ok) toast({ tone: 'bad', title: 'That change could not be applied', description: result.error });
    return result.ok;
  }, []);
}

export interface PlacedIssue extends Issue {
  severity: 'error' | 'warning';
}

// Issue paths are canonicalised once per report (index segments become @id).
const cache = new WeakMap<Report, PlacedIssue[]>();

export function placedIssues(report: Report, profile: Profile | null): PlacedIssue[] {
  let list = cache.get(report);
  if (!list) {
    const place = (severity: PlacedIssue['severity']) => (i: Issue) => ({ ...i, severity, path: canonicalPath(profile, i.path) });
    list = [...report.errors.map(place('error')), ...report.warnings.map(place('warning'))];
    cache.set(report, list);
  }
  return list;
}

export function useAllIssues(): PlacedIssue[] {
  const report = useProfileStore(selectEffectiveReport);
  const profile = useProfileStore(selectEffectiveProfile);
  return useMemo(() => placedIssues(report, profile), [report, profile]);
}

/**
 * Issues attached to a path. `deep` also includes issues on anything beneath it
 * (for a section header or a list item).
 */
export function useIssues(path: string, deep = false): FieldIssue[] {
  const all = useAllIssues();
  return useMemo(
    () =>
      all
        .filter((i) => (deep ? isWithin(i.path, path) : i.path === path))
        .map((i) => ({ severity: i.severity, message: i.message })),
    [all, path, deep],
  );
}

/** Help text for a field from the schema description. */
export function useHelp(path: string): string | undefined {
  const profile = useProfileStore(selectEffectiveProfile);
  // The description depends only on which union variants lie along the path, which
  // change rarely; recomputing per profile change is cheap (a short schema walk).
  return useMemo(() => describe(profile, path), [profile, path]);
}

export function usePeople(): { value: string; label: string }[] {
  const people = useProfile().household.people;
  return people.map((p, i) => ({ value: String(i), label: p.name || `Person ${i + 1}` }));
}
