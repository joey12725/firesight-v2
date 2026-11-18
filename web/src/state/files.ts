// Import and export of plans as JSON files.
import { EngineError, migrateV1, normalize } from '@/engine';
import type { Profile } from '@/engine/types';
import type { RunSettings, Scenario } from './profile-store';

/** A plan file with scenarios. A bare v2 profile (as in `examples/`) is also accepted. */
export interface PlanFile {
  firesight: 2;
  profile: Profile;
  scenarios: Scenario[];
  run?: RunSettings;
}

export interface ImportedPlan {
  format: 'v2' | 'v1';
  profile: Profile;
  scenarios: Scenario[];
  run?: Partial<RunSettings>;
  /** For v1 files: the assumptions the migration made. */
  notes: string[];
}

/** Reads plan JSON in any supported format. Throws an Error with a message for the user. */
export function parsePlan(text: string): ImportedPlan {
  let data: unknown;
  try {
    data = JSON.parse(text);
  } catch (error) {
    throw new Error('That file is not valid JSON.', { cause: error });
  }
  if (data === null || typeof data !== 'object' || Array.isArray(data)) {
    throw new Error('That file does not contain a FIRESight plan.');
  }
  const obj = data as Record<string, unknown>;

  const wrapped = obj.firesight === 2 && typeof obj.profile === 'object';
  const candidate = (wrapped ? obj.profile : obj) as Record<string, unknown>;
  if (wrapped || candidate.schema_version === 2 || 'household' in candidate) {
    const result = normalize(candidate);
    const schemaError = result.errors.find((e) => e.code === 'schema');
    if (schemaError) throw new Error(schemaError.message);
    return {
      format: 'v2',
      profile: result.profile,
      scenarios: wrapped && Array.isArray(obj.scenarios) ? (obj.scenarios as Scenario[]) : [],
      run: wrapped ? (obj.run as RunSettings | undefined) : undefined,
      notes: [],
    };
  }

  try {
    const migrated = migrateV1(text);
    return { format: 'v1', profile: migrated.profile, scenarios: [], notes: migrated.notes };
  } catch (error) {
    const detail = error instanceof EngineError ? ` (${error.message})` : '';
    throw new Error(`That file is not a FIRESight plan from this version or from version 1${detail}.`, { cause: error });
  }
}

export function planFileText(profile: Profile, scenarios: Scenario[], run: RunSettings): string {
  const withRun: Profile = { ...profile, simulation: { ...profile.simulation, ...run } };
  // Without scenarios, write a bare profile: the same format the CLI and examples use.
  const content = scenarios.length === 0 ? withRun : ({ firesight: 2, profile: withRun, scenarios, run } satisfies PlanFile);
  return `${JSON.stringify(content, null, 2)}\n`;
}

export function downloadText(filename: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function fileSlug(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || 'plan';
}
