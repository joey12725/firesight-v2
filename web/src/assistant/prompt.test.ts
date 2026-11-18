import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { applyPatch, initEngineSync, schema } from '@/engine/api';
import { getAt } from '@/state/pointer';
import { EXAMPLE_INTAKE_PATCH, EXAMPLE_SCENARIO_PATCH, PROMPT_MARKER, promptFingerprint, SYSTEM_PROMPT } from './prompt';
import { toolDefinitions } from './tools';
import type { SchemaNode } from './tools/plan-view';

beforeAll(() => {
  initEngineSync(readFileSync(new URL('../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));
});

describe('system prompt', () => {
  it('is frozen text that starts with the marker the proxy checks', () => {
    expect(SYSTEM_PROMPT.startsWith(PROMPT_MARKER)).toBe(true);
    // Nothing that varies between requests may be in the cached prefix.
    expect(SYSTEM_PROMPT).not.toMatch(/\b20[2-9]\d-\d\d-\d\d\b/);
    expect(SYSTEM_PROMPT).not.toContain('undefined');
    expect(SYSTEM_PROMPT).not.toContain('[object');
    // Above the minimum cacheable prefix, and small enough to read on every request.
    expect(SYSTEM_PROMPT.length).toBeGreaterThan(8_000);
    expect(SYSTEM_PROMPT.length).toBeLessThan(24_000);
  });

  it('states the rules the product depends on', () => {
    for (const phrase of ['comes from a tool result', 'natural frequencies', 'within_noise', 'not a financial adviser', 'propose_changes', '<followups>', 'account numbers', 'one life of thousands']) {
      expect(SYSTEM_PROMPT, phrase).toContain(phrase);
    }
    expect(SYSTEM_PROMPT).not.toMatch(/\p{Extended_Pictographic}/u);
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
    expect(words.length).toBeGreaterThan(80);
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
