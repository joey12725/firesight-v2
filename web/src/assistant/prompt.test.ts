import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { applyPatch, equityPreview, initEngineSync, schema } from '@/engine/api';
import { getAt } from '@/state/pointer';
import { EXAMPLE_EQUITY_PATCH, EXAMPLE_EQUITY_ZERO_PATCH, EXAMPLE_INTAKE_PATCH, EXAMPLE_SCENARIO_PATCH, PROMPT_MARKER, promptFingerprint, STAGE_FMV_FRACTIONS, SYSTEM_PROMPT } from './prompt';
import { toolDefinitions } from './tools';
import type { SchemaNode } from './tools/plan-view';

beforeAll(() => {
  initEngineSync(readFileSync(new URL('../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));
});

describe('system prompt', () => {
  it('is frozen text that starts with the marker the proxy checks', () => {
    expect(SYSTEM_PROMPT.startsWith(PROMPT_MARKER)).toBe(true);
    expect(PROMPT_MARKER).toBe('[FIRESight planning assistant, prompt v3]');
    // Nothing that varies between requests may be in the cached prefix.
    expect(SYSTEM_PROMPT).not.toMatch(/\b20[2-9]\d-\d\d-\d\d\b/);
    expect(SYSTEM_PROMPT).not.toContain('undefined');
    expect(SYSTEM_PROMPT).not.toContain('[object');
    // Above the minimum cacheable prefix, and small enough to read on every request.
    expect(SYSTEM_PROMPT.length).toBeGreaterThan(8_000);
    expect(SYSTEM_PROMPT.length).toBeLessThan(31_500);
  });

  it('states the rules the product depends on', () => {
    for (const phrase of ['comes from a tool result', 'natural frequencies', 'within_noise', 'not a financial adviser', 'propose_changes', '<followups>', 'account numbers', 'one life of thousands', 'Paper value is not money', 'with the equity counted at zero', 'exit_price_multiple', 'never give a single value']) {
      expect(SYSTEM_PROMPT, phrase).toContain(phrase);
    }
    expect(SYSTEM_PROMPT).not.toMatch(/\p{Extended_Pictographic}/u);
  });

  it('guards the ways the equity can be misreported', () => {
    for (const phrase of [
      // A custom table inherits nothing from the stage.
      'copy typical_rows from derived.private_equity',
      'never write an acquired or ipo row without years_from, years_to, multiple_low and multiple_high',
      // The default odds are judgement, not a measured base rate.
      'judgements anchored to published cohort figures, not measured rates',
      // Outcome classes are small and carry no interval.
      'do not rank outcomes on differences of a few points',
      // Under a when-funded trigger the equity moves the retirement date.
      'the equity often moves the retirement age more than success',
      // The exit price in dollars comes from the result.
      'State the price per share from value_dollars_per_share',
      // What sell_down does not govern, and that exercising now is capped.
      'policy.sell_down governs only shares owned by the end of the lockup',
      'exercising now is capped by what savings above the emergency fund can pay',
    ]) {
      expect(SYSTEM_PROMPT, phrase).toContain(phrase);
    }
    // The parenthesis that pointed at a field which cannot exist yet is gone.
    expect(SYSTEM_PROMPT).not.toContain('derived.private_equity gives it once the company exists');
    expect(SYSTEM_PROMPT).not.toMatch(/what has happened to companies at the same stage/);
  });

  it('gives worked examples that the patch engine accepts', () => {
    const blank = applyPatch('{}', []).profile;
    const built = applyPatch(blank, EXAMPLE_INTAKE_PATCH);
    expect(built.errors).toEqual([]);
    expect(getAt(built.profile, '/incomes/@lena_job/kind/plan/match_rate')).toBe(0.5);
    const scenario = applyPatch(built.profile, EXAMPLE_SCENARIO_PATCH);
    expect(scenario.errors).toEqual([]);
    expect(getAt(scenario.profile, '/retirement/trigger/age')).toBe(58);
    expect(getAt(scenario.profile, '/incomes/@lena_job/kind/plan/employee_fraction')).toBe(0.1);
    expect(SYSTEM_PROMPT).toContain(JSON.stringify(EXAMPLE_INTAKE_PATCH[0]));
  });

  it('gives startup equity examples that the engine accepts', () => {
    const blank = applyPatch('{}', []).profile;
    const intake = applyPatch(blank, EXAMPLE_INTAKE_PATCH);
    const built = applyPatch(intake.profile, EXAMPLE_EQUITY_PATCH);
    expect(built.errors).toEqual([]);
    // The position is far larger than the 401(k), which is the one warning the equity adds.
    const before = new Set(intake.warnings.map((w) => w.code));
    expect(built.warnings.map((w) => w.code).filter((code) => !before.has(code))).toEqual(['equity_dominant']);
    const company = built.profile.private_equity[0]!;
    expect(company).toMatchObject({ id: 'acme', job: 'lena_job', stage: 'series_b', included: true, outcomes: [] });
    expect(company.holdings[0]).toMatchObject({ shares: 120000, vested: 90000, kind: { type: 'iso', strike: 0.8 } });
    // The example's prose and its patch say the same thing.
    expect(SYSTEM_PROMPT).toContain('120,000 ISOs at 80 cents');
    expect(SYSTEM_PROMPT).toContain(JSON.stringify(EXAMPLE_EQUITY_PATCH[0]));
    expect(SYSTEM_PROMPT).toContain(JSON.stringify(EXAMPLE_EQUITY_ZERO_PATCH[0]));

    const zero = applyPatch(built.profile, EXAMPLE_EQUITY_ZERO_PATCH);
    expect(zero.errors).toEqual([]);
    expect(getAt(zero.profile, '/private_equity/@acme/included')).toBe(false);
    // Counted at nothing, the position no longer dominates anything.
    expect(zero.warnings.map((w) => w.code)).not.toContain('equity_dominant');
  });

  it('quotes the 409A fractions and the example odds the engine uses', () => {
    const stages = Object.keys(STAGE_FMV_FRACTIONS) as (keyof typeof STAGE_FMV_FRACTIONS)[];
    expect(stages).toEqual(['seed', 'series_a', 'series_b', 'series_c', 'late', 'unicorn']);
    const root = schema() as SchemaNode;
    const inSchema = (root.$defs?.CompanyStage?.oneOf ?? root.$defs?.CompanyStage?.anyOf ?? []).map((m) => m.const ?? m.enum?.[0]);
    expect(root.$defs?.CompanyStage?.enum ?? inSchema).toEqual(stages);
    for (const stage of stages) {
      // With no 409A value given, the preview reports the stage's typical one: the fraction times the price.
      expect(equityPreview({ stage, preferred_price: 1 }).fmv_409a, stage).toBeCloseTo(STAGE_FMV_FRACTIONS[stage], 12);
    }
    expect(SYSTEM_PROMPT).toContain('seed 0.20, A 0.25, B 0.30, C 0.35, later 0.45, above a billion 0.65');
    // The odds quoted as an example of how to state them are a real preset (Series C).
    const odds = Object.fromEntries(equityPreview({ stage: 'series_c', preferred_price: 1 }).typical.map((row) => [row.kind, Math.round(row.probability * 100)]));
    expect(odds).toEqual({ fails: 30, acquired: 27, ipo: 12, still_private: 31 });
    expect(SYSTEM_PROMPT).toContain('failing in about 30 of 100 futures over the next ten years, acquired in 27, public in 12, and still private in 31');
  });

  it('names only fields that exist in the schema map', () => {
    const root = schema() as SchemaNode;
    const known = new Set<string>(Object.keys(root.properties ?? {}));
    const collect = (node: SchemaNode | undefined) => {
      if (!node) return;
      for (const [key, child] of Object.entries(node.properties ?? {})) {
        known.add(key);
        if (typeof child.const === 'string') known.add(child.const);
        collect(child);
      }
      for (const alt of [...(node.oneOf ?? []), ...(node.anyOf ?? [])]) {
        if (typeof alt.const === 'string') known.add(alt.const);
        collect(alt);
      }
      for (const value of node.enum ?? []) if (typeof value === 'string') known.add(value);
      collect(node.items);
    };
    for (const def of Object.values(root.$defs ?? {})) collect(def);
    const map = SYSTEM_PROMPT.slice(SYSTEM_PROMPT.indexOf('Profile (top level)'), SYSTEM_PROMPT.indexOf('This map names every field'));
    // Every snake_case word in the map is a real field, variant or enum value.
    const words = [...new Set(map.match(/\b[a-z]+(?:_[a-z0-9]+)+\b/g) ?? [])];
    expect(words.length).toBeGreaterThan(110);
    for (const word of ['private_equity', 'preferred_price', 'fmv_409a', 'tender_sell_fraction', 'leave_after_months', 'still_private', 'at_liquidity']) expect(words, word).toContain(word);
    expect(words.filter((w) => !known.has(w))).toEqual([]);
    // And every top-level section is mentioned.
    for (const key of Object.keys(root.properties ?? {})) {
      if (key === 'schema_version' || key === 'description' || key === 'simulation') continue;
      expect(map, key).toContain(key);
    }
  });

  it('fingerprints the prompt and tools together', () => {
    const tools = toolDefinitions();
    expect(promptFingerprint(tools)).toBe(promptFingerprint(toolDefinitions()));
    expect(promptFingerprint([...tools.slice(1)])).not.toBe(promptFingerprint(tools));
  });
});
