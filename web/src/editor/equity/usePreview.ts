// The engine's standalone preview of one company, for the editor: cached by the
// company's JSON (the preview is deterministic), and debounced while a field is typed.
import { useEffect, useState } from 'react';
import { equityPreview } from '@/engine/api';
import type { EquityPreview } from '@/engine/equity-types';
import type { CompanyStage, PrivateCompany } from '@/engine/types';

const DEBOUNCE_MS = 80;
const CACHE_SIZE = 48;
const cache = new Map<string, EquityPreview | null>();

function compute(json: string): EquityPreview | null {
  if (cache.has(json)) return cache.get(json) ?? null;
  let preview: EquityPreview | null;
  try {
    preview = equityPreview(json, 0);
  } catch {
    // The engine is not loaded, or the company is mid-edit in a state it cannot read: show nothing.
    preview = null;
  }
  if (cache.size >= CACHE_SIZE) cache.delete(cache.keys().next().value as string);
  cache.set(json, preview);
  return preview;
}

/** The preview of a company, or null when the engine cannot give one. Synchronous and cached. */
export function previewOf(company: PrivateCompany): EquityPreview | null {
  return compute(JSON.stringify(company));
}

/** True when the preview's payoffs mean something: there is a price and something is held. */
export function canShowPayoffs(company: Pick<PrivateCompany, 'preferred_price' | 'holdings'>): boolean {
  return company.preferred_price > 0 && company.holdings.length > 0;
}

/**
 * The preview of a company as it is edited. The first value is computed at once; later
 * changes are debounced 80 ms, so typing a figure does not run the engine per keystroke.
 * Null when the engine cannot read the company.
 */
export function usePreview(company: PrivateCompany): EquityPreview | null {
  const json = JSON.stringify(company);
  const [shown, setShown] = useState(() => ({ json, preview: compute(json) }));
  useEffect(() => {
    if (shown.json === json) return;
    const timer = setTimeout(() => setShown({ json, preview: compute(json) }), DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [json, shown.json]);
  return shown.preview;
}

export interface StageDefaults {
  /** Typical 409A value of a common share as a fraction of the preferred price. */
  fmvRatio: number;
  /** Typical money raised as a fraction of the latest valuation. */
  overhang: number;
  /** Typical chance of a tender offer per year. */
  tender: number;
}

const stages = new Map<CompanyStage, StageDefaults | null>();

/** A stage's typical values, read from the engine (a bare company at a price of 1), so no constant is copied here. */
export function stageDefaults(stage: CompanyStage): StageDefaults | null {
  if (!stages.has(stage)) {
    try {
      const p = equityPreview({ stage, preferred_price: 1 }, 100);
      stages.set(stage, { fmvRatio: p.fmv_409a, overhang: p.preference_overhang, tender: p.tender_annual_probability });
    } catch {
      return null;
    }
  }
  return stages.get(stage) ?? null;
}
