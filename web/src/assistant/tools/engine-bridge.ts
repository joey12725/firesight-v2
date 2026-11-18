// The real engine behind the tools: the worker pool for simulation, and the wasm
// module's main-thread calls for everything else.
//
// `compare` and `planPreview` are being added to the wasm wrapper; they are looked up
// on the module at call time, so the tools use core's exact paired statistics and plan
// preview as soon as a build exports them, and the TypeScript fallbacks until then.
import { applyPatch, getPool, levers, schema } from '@/engine';
import * as wasm from '@/engine/pkg/firesight_wasm.js';
import type { Comparison } from '@/engine/output-types';
import type { EngineBridge } from './types';

const optional = wasm as unknown as Record<string, unknown>;

function callJson<T>(name: string, ...args: unknown[]): T | null {
  const fn = optional[name];
  if (typeof fn !== 'function') return null;
  const out = (fn as (...a: unknown[]) => unknown)(...args);
  return typeof out === 'string' ? (JSON.parse(out) as T) : (out as T);
}

let cachedSchema: unknown;

export function realEngine(): EngineBridge {
  return {
    get pool() {
      return getPool();
    },
    applyPatch,
    levers,
    schema: () => (cachedSchema ??= schema()),
    compare: (a, b) => callJson<Comparison>('compare', a.acc, b.acc, JSON.stringify(a.profile)),
    planPreview: (profile) => callJson<unknown>('planPreview', JSON.stringify(profile)),
  };
}
