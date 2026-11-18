import type { Comparison } from '@/engine/output-types';
import { realEngine } from './engine-bridge';
import type { RunRecord } from './runs';

/** Core's full paired comparison for a chart, or null when this wasm build has no `compare`. */
export function compareRunsExact(a: RunRecord, b: RunRecord): Comparison | null {
  try {
    return realEngine().compare?.(a, b) ?? null;
  } catch {
    return null;
  }
}
