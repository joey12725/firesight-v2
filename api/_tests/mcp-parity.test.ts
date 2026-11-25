// The hosted MCP server and the website must tell an agent the same things about the
// same plan. The server cannot import the website's modules into a function (they use
// the app's path aliases), so it keeps its own copies of a few pure functions. This
// file loads the website's originals and holds the copies equal to them, on the
// engine's own example households.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import * as wasm from '../_engine/firesight_wasm.js';
import { applyPatch, compare, currentYearMonth, digest, engineInput, normalise, planPreview, schema, simulate } from '../_lib/engine.js';
import type { PatchOp, Profile, Report, Run } from '../_lib/engine.js';
import { digestComparison, digestRun } from '../_lib/mcp/digest.js';
import { HOSTED_VOCABULARY, SCHEMA_MAP, SITE_VIEWS, guide } from '../_lib/mcp/guide.js';
import { compactPatch, describeRoot, describeType, planView, pruneDefaults, referencedTypes, typeNames, uniqueId } from '../_lib/mcp/view.js';
import type { Scenario, SchemaNode } from '../_lib/mcp/view.js';

const web = (path: string): string => fileURLToPath(new URL(`../../web/src/${path}`, import.meta.url));

// Loaded by path at run time, so the server's type check does not reach into the web
// app. Each is described here by the part of it this file calls.
interface WebPlanView {
  planView(input: unknown): unknown;
  pruneDefaults(root: unknown, node: unknown, value: unknown): unknown;
  describeType(root: unknown, name: string): unknown;
  describeRoot(root: unknown): unknown;
  referencedTypes(root: unknown, name: string): string[];
  typeNames(root: unknown): string[];
}
interface WebDigest {
  digestRun(engine: { digest(run: unknown): unknown }, run: unknown): Record<string, unknown>;
  digestComparison(comparison: unknown, seed: number): unknown;
}
interface WebPool {
  engineInput(profile: unknown, seed?: number): { json: string; seed: number };
}
interface WebPointer {
  uniqueId(name: string, taken: Iterable<string | undefined>): string;
}
interface WebPatches {
  compactPatch(ops: PatchOp[]): PatchOp[];
}
interface WebGuide {
  SCHEMA_MAP: string;
  guideEssentials(vocabulary?: unknown): string;
}

const load = async <T>(path: string): Promise<T> => (await import(/* @vite-ignore */ web(path))) as T;

const examples = (JSON.parse(wasm.exampleProfiles()) as { slug: string; profile: Profile }[]).map((example) => {
  const result = normalise(example.profile);
  return { slug: example.slug, profile: result.profile, report: { errors: result.errors, warnings: result.warnings } satisfies Report };
});
const root = schema() as SchemaNode;

describe('the plan as an agent reads it', () => {
  it('has examples to compare on', () => {
    expect(examples.length).toBeGreaterThanOrEqual(3);
  });

  it('is the same compact view the website gives', async () => {
    const theirs = await load<WebPlanView>('assistant/tools/plan-view.ts');
    const scenarios: Scenario[] = [
      { id: 'retire_later', name: 'Retire later', enabled: true, patch: [{ op: 'set', path: '/retirement/bequest_target', value: 100_000 }] },
      { id: 'many', name: 'Many edits', enabled: false, patch: Array.from({ length: 12 }, (_, i) => ({ op: 'set' as const, path: '/retirement/bequest_target', value: i })) },
    ];
    for (const example of examples) {
      const input = { profile: example.profile, report: example.report, scenarios, run: { paths: 4_000, seed: 7 }, schema: root, today: currentYearMonth(), preview: planPreview(example.profile) };
      expect(planView(input), example.slug).toEqual(theirs.planView(input));
      expect(pruneDefaults(root, { properties: root.properties }, example.profile), example.slug).toEqual(theirs.pruneDefaults(root, { properties: root.properties }, example.profile));
    }
    // A plan that cannot run yet: no preview, and the errors are listed.
    const empty = normalise({});
    const input = { profile: empty.profile, report: { errors: empty.errors, warnings: empty.warnings }, scenarios: [], run: { paths: 10_000, seed: 1 }, schema: root, today: '2026-11', preview: null };
    expect(planView(input)).toEqual(theirs.planView(input));
    expect(planView(input).validation.can_run).toBe(false);
    expect(planView(input).empty_plan).toBe(true);
  });

  it('describes every schema type the way the website does', async () => {
    const theirs = await load<WebPlanView>('assistant/tools/plan-view.ts');
    expect(typeNames(root)).toEqual(theirs.typeNames(root));
    expect(typeNames(root).length).toBeGreaterThan(50);
    expect(describeRoot(root)).toEqual(theirs.describeRoot(root));
    for (const name of typeNames(root)) {
      expect(describeType(root, name), name).toEqual(theirs.describeType(root, name));
      expect(referencedTypes(root, name), name).toEqual(theirs.referencedTypes(root, name));
    }
    expect(describeType(root, 'NoSuchType')).toBeNull();
  });

  it('names scenarios and tidies patches as the website does', async () => {
    const pointer = await load<WebPointer>('state/pointer.ts');
    const patches = await load<WebPatches>('state/patches.ts');
    for (const [name, taken] of [
      ['Retire at 55', ['base']],
      ['Retire at 55', ['base', 'retire_at_55', 'retire_at_55_2']],
      ['  ¡¿?!  ', []],
      ['A name that is much longer than thirty-two characters in total', ['x']],
    ] as const) {
      expect(uniqueId(name, taken)).toBe(pointer.uniqueId(name, taken));
    }
    const ops: PatchOp[] = [
      { op: 'set', path: '/a', value: 1 },
      { op: 'set', path: '/b', value: 2 },
      { op: 'set', path: '/a', value: 3 },
      { op: 'append', path: '/list', value: { id: 'x' } },
      { op: 'set', path: '/a', value: 4 },
      { op: 'remove', path: '/list/@x' },
      { op: 'set', path: '/a', value: 5 },
    ];
    expect(compactPatch(ops)).toEqual(patches.compactPatch(ops));
    expect(compactPatch(ops)).toHaveLength(6);
  });
});

describe('figures', () => {
  let base: Run;
  let changed: Run;
  const example = examples[0];
  if (!example) throw new Error('The engine has no example profiles.');

  it('compiles a profile exactly as the website does, so a seed means the same lives', async () => {
    const pool = await load<WebPool>('engine/pool.ts');
    for (const { profile } of examples) {
      expect(engineInput(profile)).toEqual({ json: pool.engineInput(profile).json, seed: pool.engineInput(profile).seed });
      expect(engineInput(profile, 42).json).toBe(pool.engineInput(profile, 42).json);
    }
  });

  it('digests a run into the figures the website tools report', async () => {
    const theirs = await load<WebDigest>('assistant/tools/digest.ts');
    base = await simulate(example.profile, { paths: 300, seed: 5 });
    const summary = JSON.parse(wasm.summarize(base.acc, base.json)) as unknown;
    const { run_id: _id, ...expected } = theirs.digestRun({ digest: () => digest(base) }, { id: 'run_1', seed: base.seed, summary, acc: base.acc });
    expect(digestRun(base)).toEqual(expected);
    expect(digestRun(base)).toMatchObject({ seed: 5, paths: 300 });
  });

  it('digests a paired comparison the same way', async () => {
    const theirs = await load<WebDigest>('assistant/tools/digest.ts');
    const later = applyPatch(example.profile, [{ op: 'set', path: '/retirement/bequest_target', value: 250_000 }]).profile;
    changed = await simulate(later, { paths: 300, seed: 5 });
    const comparison = compare(base, changed);
    expect(digestComparison(comparison, 5)).toEqual(theirs.digestComparison(comparison, 5));
    expect(digestComparison(comparison, 5)).toMatchObject({ paired: true, paired_paths: 300, seed: 5 });
  });
});

describe('the guide', () => {
  it('is the website guide with this server\'s tool names', async () => {
    const theirs = await load<WebGuide>('webmcp/guide.ts');
    expect(SCHEMA_MAP).toBe(theirs.SCHEMA_MAP);
    expect(guide()).toBe(theirs.guideEssentials(HOSTED_VOCABULARY));
    expect(guide()).toContain('update_plan');
    expect(guide()).toContain('compare_scenario');
    // The in-page names that differ here must not leak into the hosted guide.
    for (const text of [guide(), guide('patches'), guide('schema'), guide('results'), guide('limits'), guide('start')]) {
      expect(text).not.toMatch(/\b(edit_plan|try_scenario|get_schema)\b/);
      expect(text.length).toBeLessThan(8_000);
    }
  });

  it('links to views the website has', () => {
    const registry = readFileSync(web('views/registry.ts'), 'utf8');
    const ids = [...registry.matchAll(/\{ id: '([a-z-]+)', label:/g)].map((match) => match[1]);
    expect(ids.length).toBeGreaterThan(0);
    expect([...SITE_VIEWS]).toEqual(ids);
  });
});
