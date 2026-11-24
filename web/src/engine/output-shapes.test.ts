// Runs the real wasm engine in Node on every demo household and checks each document
// it produces against the shapes in `output-shapes.ts` (which the compiler ties to the
// types in `output-types.ts`). A field renamed, added or removed on the Rust side, or a
// number that turns out to be nullable, fails here with the JSON path.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import * as wasm from './pkg/firesight_wasm.js';
import { comparisonShape, digestShape, pathTraceShape, planPreviewShape, summaryShape } from './output-shapes';
import type { PathTrace, Summary } from './output-types';
import { shapeIssues } from './shape';
import type { ExampleProfile, Lever } from './types';

const PATHS = 500;
let examples: ExampleProfile[] = [];

beforeAll(() => {
  wasm.initSync({ module: readFileSync(new URL('./pkg/firesight_wasm_bg.wasm', import.meta.url)) });
  examples = JSON.parse(wasm.exampleProfiles()) as ExampleProfile[];
});

function run(profileJson: string, paths = PATHS) {
  const engine = new wasm.Engine(profileJson);
  const merger = new wasm.Merger();
  // Two chunks, so the merge path is exercised too.
  merger.add(engine.runChunk(0, paths / 2));
  merger.add(engine.runChunk(paths / 2, paths / 2));
  return { engine, merger };
}

describe('engine output matches the TypeScript types', () => {
  it('has the four demo households', () => {
    expect(examples.map((e) => e.slug).sort()).toEqual(['dual-income-family', 'early-career-saver', 'full-life', 'side-hustle-landlord']);
  });

  for (const slug of ['early-career-saver', 'dual-income-family', 'side-hustle-landlord', 'full-life']) {
    describe(slug, () => {
      it('Summary, digest, PathTrace, PlanPreview and Comparison conform', () => {
        const profile = examples.find((e) => e.slug === slug)?.profile;
        expect(profile).toBeDefined();
        const json = JSON.stringify(profile);
        const { engine, merger } = run(json);
        expect(merger.paths()).toBe(PATHS);

        const summary = JSON.parse(merger.summarize(json)) as Summary;
        expect(shapeIssues(summaryShape, summary)).toEqual([]);
        expect(summary.paths).toBe(PATHS);
        // The invariants the charts lean on.
        const years = summary.axis.years.length;
        expect(summary.axis.ages).toHaveLength(years);
        expect(summary.axis.alive).toHaveLength(years);
        for (const q of summary.quantiles) for (const row of q.values) expect(row).toHaveLength(years);
        for (const d of summary.density) {
          expect(d.counts).toHaveLength(years);
          expect(d.starts).toHaveLength(years);
          for (let y = 0; y < years; y++) expect((d.starts[y] ?? 0) + (d.counts[y]?.length ?? 0)).toBeLessThan(d.edges.length + 1);
        }
        for (let y = 0; y < years; y++) {
          const total = summary.status.shares.reduce((sum, row) => sum + (row[y] ?? 0), 0);
          expect(total).toBeCloseTo(1, 3);
        }
        expect(summary.status.statuses).toEqual(['working', 'retired_surplus', 'retired_on_plan', 'retired_cutting', 'depleted', 'dead']);

        // The stand-alone `summarize` / `digest` agree with the Merger's.
        const bytes = merger.bytes();
        expect(JSON.parse(wasm.summarize(bytes, json))).toEqual(summary);
        const digest: unknown = JSON.parse(merger.digest(json));
        expect(shapeIssues(digestShape, digest)).toEqual([]);
        expect(JSON.parse(wasm.digest(bytes, json))).toEqual(digest);

        expect(shapeIssues(planPreviewShape, JSON.parse(wasm.planPreview(json)))).toEqual([]);

        for (const pick of summary.picks.slice(0, 2)) {
          const trace = JSON.parse(engine.trace(pick.path_index)) as PathTrace;
          expect(shapeIssues(pathTraceShape, trace)).toEqual([]);
          expect(trace.path_index).toBe(pick.path_index);
          expect(trace.status).toHaveLength(trace.months);
        }

        // A scenario on the same paths: the first sensitivity lever, low side.
        const lever = (JSON.parse(wasm.levers(json)) as Lever[])[0];
        expect(lever).toBeDefined();
        const patched = (JSON.parse(wasm.applyPatch(json, JSON.stringify(lever?.low))) as { profile: unknown }).profile;
        const other = run(JSON.stringify(patched));
        const comparison: unknown = JSON.parse(wasm.compare(bytes, other.merger.bytes(), json));
        expect(shapeIssues(comparisonShape, comparison)).toEqual([]);
      }, 60_000);
    });
  }

  it('a tiny run still conforms (nulls where nothing happened)', () => {
    const json = JSON.stringify(examples[0]?.profile);
    const engine = new wasm.Engine(json);
    const merger = new wasm.Merger();
    merger.add(engine.runChunk(0, 3));
    expect(shapeIssues(summaryShape, JSON.parse(merger.summarize(json)))).toEqual([]);
    expect(shapeIssues(digestShape, JSON.parse(merger.digest(json)))).toEqual([]);
  });
});
