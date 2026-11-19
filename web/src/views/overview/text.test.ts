import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import * as wasm from '@/engine/pkg/firesight_wasm.js';
import type { Summary } from '@/engine/output-types';
import type { ExampleProfile } from '@/engine/types';
import { failureTakeaway, fanTakeaway, fiTakeaway, homeSaleNote, homeSaleSentence, lives, outcomeSentence, plain, samplingNote, spendingTakeaway, statusTakeaway } from './text';

let summaries: { slug: string; summary: Summary }[] = [];

function run(profile: unknown, paths: number): Summary {
  const json = JSON.stringify(profile);
  const merger = new wasm.Merger();
  merger.add(new wasm.Engine(json).runChunk(0, paths));
  return JSON.parse(merger.summarize(json)) as Summary;
}

beforeAll(() => {
  wasm.initSync({ module: readFileSync(new URL('../../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)) });
  const examples = JSON.parse(wasm.exampleProfiles()) as ExampleProfile[];
  summaries = examples.map((e) => ({ slug: e.slug, summary: run(e.profile, 400) }));
});

describe('overview sentences on real engine output', () => {
  it('counts add up and read as natural frequencies', () => {
    for (const { summary } of summaries) {
      const text = plain(outcomeSentence(summary));
      expect(text).toMatch(/^In (all )?\d+ of 100 simulated lives the plan holds to the end\./);
      expect(text).not.toMatch(/NaN|null|undefined|Infinity/);
      const holds = Number(/In (\d+) of 100/.exec(text)?.[1] ?? 100);
      expect(holds).toBe(Math.round(summary.headline.success_rate.value * 100));
    }
  });

  it('every takeaway is a complete sentence with no missing values', () => {
    for (const { summary } of summaries) {
      for (const text of [samplingNote(summary), fiTakeaway(summary), fanTakeaway(summary, 'net_worth'), fanTakeaway(summary, 'investable'), statusTakeaway(summary), spendingTakeaway(summary), failureTakeaway(summary)]) {
        expect(text).not.toMatch(/NaN|null|undefined|Infinity/);
        expect(text.trim()).toMatch(/[.)]$/);
        expect(text.length).toBeGreaterThan(20);
      }
    }
  });

  it('puts the last-resort home sale beside the success figure and never headlines more than 100% delivered', () => {
    for (const { summary } of summaries) {
      const rate = summary.headline.home_sale_rate.value;
      const sale = homeSaleSentence(summary);
      if (rate >= 0.005) {
        expect(plain(sale ?? [])).toMatch(/^Holding includes selling a home as a last resort, which happens in \d+ of 1,?0?00 lives/);
        expect(homeSaleNote(summary)).toMatch(/^a home is sold as a last resort in \d+ of/);
      } else {
        expect(sale).toBeNull();
      }
      expect(summary.headline.plan_delivered_share.value ?? 0).toBeLessThanOrEqual(1);
      const delivered = /(\d+)% of planned retirement spending is delivered/.exec(spendingTakeaway(summary));
      if (delivered) expect(Number(delivered[1])).toBeLessThanOrEqual(100);
    }
  });

  it('uses the lives actually simulated when there are fewer than 100', () => {
    expect(lives(0.5, 20)).toEqual({ n: '10', of: '20' });
    expect(lives(0.834, 20_000)).toEqual({ n: '83', of: '100' });
    expect(lives(0.004, 20_000)).toEqual({ n: '4', of: '1,000' });
    const tiny = run((JSON.parse(wasm.exampleProfiles()) as ExampleProfile[])[1]?.profile, 7);
    expect(plain(outcomeSentence(tiny))).toMatch(/of 7 simulated lives|all 7 simulated lives/);
    expect(fiTakeaway(tiny)).not.toMatch(/NaN|null|undefined/);
  });
});
