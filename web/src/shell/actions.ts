// User-level actions shared by the top bar, the landing page and dialogs.
import { exampleProfiles, defaultProfile } from '@/engine';
import { fileSlug, downloadText, parsePlan, planFileText } from '@/state/files';
import { createHousehold } from '@/state/households';
import { useProfileStore } from '@/state/profile-store';
import { decodeShare, shareParam } from '@/state/share';
import { toast } from '@/state/ui-store';

/** The methodology: what the model does, with its evidence and its known limits. */
export const METHODOLOGY_URL = 'https://github.com/joey12725/firesight-v2/blob/main/docs/methodology.md';

export function openExample(slug: string) {
  const example = exampleProfiles().find((e) => e.slug === slug);
  if (!example) return;
  createHousehold({ profile: example.profile, origin: `demo:${slug}` });
}

export function openBlank() {
  createHousehold({ profile: defaultProfile(), origin: 'blank' });
}

/** Imports plan JSON (v2, v2 with scenarios, or v1). Returns false and toasts on failure. */
export function importPlanText(text: string, origin = 'import'): boolean {
  try {
    const plan = parsePlan(text);
    createHousehold({ profile: plan.profile, scenarios: plan.scenarios, run: plan.run, origin, migrationNotes: plan.notes });
    if (plan.format === 'v2') toast({ tone: 'good', title: `Imported "${plan.profile.name || 'plan'}"` });
    return true;
  } catch (error) {
    toast({ tone: 'bad', title: 'Could not import that file', description: error instanceof Error ? error.message : String(error) });
    return false;
  }
}

export async function importPlanFile(file: File) {
  return importPlanText(await file.text());
}

export function exportCurrentPlan() {
  const s = useProfileStore.getState();
  if (!s.profile) return;
  downloadText(`${fileSlug(s.profile.name)}.firesight.json`, planFileText(s.profile, s.scenarios, s.run));
}

/** Opens the plan carried in the URL fragment, if any, and clears the fragment. */
export async function openSharedPlan(): Promise<boolean> {
  const encoded = shareParam();
  if (!encoded) return false;
  window.history.replaceState(null, '', window.location.pathname + window.location.search);
  try {
    const payload = await decodeShare(encoded);
    createHousehold({ profile: payload.profile, scenarios: payload.scenarios, run: payload.run, origin: 'share' });
    toast({ tone: 'good', title: 'Opened a shared plan', description: 'It is now saved in this browser as its own household.' });
    return true;
  } catch (error) {
    toast({ tone: 'bad', title: 'That share link could not be read', description: error instanceof Error ? error.message : String(error) });
    return false;
  }
}
