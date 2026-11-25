// Renderings of a plan and of the plan schema for an agent: short, faithful, and always
// carrying the ids that patches address elements by.
//
// These are the server's copies of what the in-app assistant and the in-page tools use
// (web/src/assistant/tools/plan-view.ts). The web modules cannot be imported from a
// function (they use the app's path aliases), so the logic is repeated here and
// api/_tests/mcp-parity.test.ts holds the two equal on the example plans.

import type { Issue, PatchOp, PlanPreview, Profile, Report } from '../engine.js';
import { companiesView, EQUITY_NOTE, type EquityPreview } from './equity-view.js';

export interface SchemaNode {
  $ref?: string;
  description?: string;
  type?: string | string[];
  const?: unknown;
  enum?: unknown[];
  default?: unknown;
  properties?: Record<string, SchemaNode>;
  additionalProperties?: SchemaNode | boolean;
  items?: SchemaNode;
  oneOf?: SchemaNode[];
  anyOf?: SchemaNode[];
  $defs?: Record<string, SchemaNode>;
}

/** A named what-if saved with a plan: edits that turn the baseline into the scenario. */
export interface Scenario {
  id: string;
  name: string;
  patch: PatchOp[];
  /** Enabled scenarios are simulated beside the baseline on the website. */
  enabled: boolean;
}

const refName = (ref: string) => ref.replace('#/$defs/', '');

function deref(root: SchemaNode, node: SchemaNode | undefined): SchemaNode | undefined {
  let n = node;
  for (let i = 0; n?.$ref && i < 8; i++) n = root.$defs?.[refName(n.$ref)];
  return n;
}

function alternatives(root: SchemaNode, node: SchemaNode): SchemaNode[] {
  const members = node.oneOf ?? node.anyOf;
  if (!members) return [node];
  return members.flatMap((m) => {
    const d = deref(root, m);
    return d && d.type !== 'null' ? alternatives(root, d) : [];
  });
}

function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  return ka.length === kb.length && ka.every((k) => deepEqual((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]));
}

const ALWAYS = new Set(['id', 'type']);

/**
 * The value with every field that still holds its schema default removed. Ids and
 * variant tags are always kept. What is left is what makes this plan this plan.
 */
export function pruneDefaults(root: SchemaNode, node: SchemaNode | undefined, value: unknown): unknown {
  const resolved = deref(root, node);
  if (Array.isArray(value)) {
    const items = resolved ? alternatives(root, resolved).find((a) => a.items)?.items : undefined;
    return value.map((el) => pruneDefaults(root, items, el));
  }
  if (value === null || typeof value !== 'object') return value;
  const record = value as Record<string, unknown>;
  const options = resolved ? alternatives(root, resolved).filter((a) => a.properties) : [];
  const chosen = options.find((o) => o.properties?.type?.const !== undefined && o.properties.type.const === record.type) ?? options[0];
  const out: Record<string, unknown> = {};
  for (const [key, child] of Object.entries(record)) {
    const schema = chosen?.properties?.[key];
    if (!ALWAYS.has(key) && schema && 'default' in schema && deepEqual(child, schema.default)) continue;
    if (!ALWAYS.has(key) && !(schema && 'default' in schema) && (child === null || child === '')) continue;
    out[key] = pruneDefaults(root, schema, child);
  }
  return out;
}

function ageAt(birth: unknown, today: string): number | null {
  if (typeof birth !== 'string') return null;
  const [by, bm] = birth.split('-').map(Number);
  const [ty, tm] = today.split('-').map(Number);
  if (!by || !bm || !ty || !tm) return null;
  return Math.floor((ty * 12 + tm - (by * 12 + bm)) / 12);
}

const issue = (i: Issue) => ({ code: i.code, path: i.path, message: i.message });

export interface ValidationView {
  can_run: boolean;
  errors: Issue[];
  warnings: Issue[];
  truncated?: true;
}

/** The validation report as an agent reads it: errors block simulation, warnings are worth raising. */
export function validationView(report: Report): ValidationView {
  return {
    can_run: report.errors.length === 0,
    errors: report.errors.slice(0, 20).map(issue),
    warnings: report.warnings.slice(0, 20).map(issue),
    ...(report.errors.length + report.warnings.length > 40 ? { truncated: true as const } : {}),
  };
}

const whole = (v: number) => Math.round(v);
const tenth = (v: number) => Math.round(v * 10) / 10;

/** The engine's plan preview as an agent reads it: whole dollars, rates to three places. */
function previewView(p: PlanPreview) {
  return {
    annual_spending: whole(p.annual_spending),
    annual_spending_retired: whole(p.annual_spending_retired),
    annual_income: whole(p.annual_income),
    annual_tax: whole(p.annual_tax),
    savings_rate: Math.round(p.savings_rate * 1000) / 1000,
    investable: whole(p.investable),
    wealth: whole(p.wealth),
    funded_requirement: whole(p.funded_requirement),
    funded_ratio: p.funded_ratio === null ? null : Math.round(p.funded_ratio * 1000) / 1000,
    retirement_need: whole(p.retirement_need),
    retirement_tax: whole(p.retirement_tax),
    future_income_value: whole(p.future_income_value),
    horizon_years: p.horizon_years,
    life_expectancy: p.life_expectancy.map(tenth),
    expected_age_at_death: p.expected_age_at_death.map(tenth),
  };
}

const list = (value: unknown): Record<string, unknown>[] => (Array.isArray(value) ? (value as Record<string, unknown>[]) : []);
const record = (value: unknown): Record<string, unknown> => (typeof value === 'object' && value !== null && !Array.isArray(value) ? (value as Record<string, unknown>) : {});
const number = (value: unknown): number => (typeof value === 'number' && Number.isFinite(value) ? value : 0);

export interface PlanViewInput {
  /** A complete profile, as the engine normalises it. */
  profile: Profile;
  report: Report;
  scenarios: Scenario[];
  run: { paths: number; seed: number };
  schema: SchemaNode;
  /** "YYYY-MM". */
  today: string;
  /** The engine's plan preview; null when the plan cannot be compiled. */
  preview: PlanPreview | null;
  /** The engine's preview of one private company. Left out, or throwing, the odds are omitted and only the paper figures are shown. */
  equityPreview?: (company: Record<string, unknown>) => EquityPreview;
}

/** The compact view of a plan: every non-default field, what the plan implies, and its validation report. */
export function planView(input: PlanViewInput) {
  const { profile, report, scenarios, schema } = input;
  const { simulation, schema_version: _version, ...rest } = profile;
  const plan = pruneDefaults(schema, { properties: schema.properties }, rest) as Record<string, unknown>;
  const household = record(profile.household);
  const accounts = list(profile.accounts);
  const byKind: Record<string, number> = {};
  for (const account of accounts) {
    const kind = String(account.kind);
    byKind[kind] = (byKind[kind] ?? 0) + number(account.balance);
  }
  const propertyEquity = list(profile.properties).reduce((n, p) => n + number(p.value) - number(record(p.mortgage).balance), 0);
  const debt = list(profile.debts).reduce((n, d) => n + number(d.balance), 0);
  const balances = accounts.reduce((n, a) => n + number(a.balance), 0);
  const companies = companiesView(profile, balances + propertyEquity - debt, input.equityPreview);
  const empty = list(household.people).length === 0 && list(profile.incomes).length === 0 && accounts.length === 0 && list(record(profile.spending).items).length === 0;
  return {
    today: input.today,
    empty_plan: empty,
    note: "Fields not shown hold their schema defaults. Amounts are in today's dollars; rates are decimals. Address list elements in patches as @id.",
    plan,
    derived: {
      ages_today: list(household.people).map((p) => ({ id: p.id, age: ageAt(p.birth, input.today) })),
      accounts_by_kind: byKind,
      property_equity: Math.round(propertyEquity),
      non_mortgage_debt: Math.round(debt),
      ...(companies.length > 0 ? { private_equity: companies, private_equity_note: EQUITY_NOTE } : {}),
    },
    ...(input.preview ? { preview: previewView(input.preview) } : {}),
    validation: validationView(report),
    scenarios: scenarios.map((s) => ({ id: s.id, name: s.name, enabled: s.enabled, ops: s.patch.length <= 10 ? s.patch : { count: s.patch.length, first: s.patch.slice(0, 10) } })),
    run_settings: { paths: input.run.paths, seed: input.run.seed, end_age: record(simulation).end_age },
  };
}

// ---- schema lookups -------------------------------------------------------------------

function typeText(root: SchemaNode, node: SchemaNode): string {
  if (node.$ref) return refName(node.$ref);
  if (node.const !== undefined) return JSON.stringify(node.const);
  if (node.enum) return node.enum.map((v) => JSON.stringify(v)).join(' | ');
  const members = node.oneOf ?? node.anyOf;
  if (members) return members.map((m) => typeText(root, m)).join(' | ');
  const type = Array.isArray(node.type) ? node.type.join(' | ') : node.type;
  if (type === 'array') return `${node.items ? typeText(root, node.items) : 'any'}[]`;
  if (type === 'object' && typeof node.additionalProperties === 'object') return `{ [key]: ${typeText(root, node.additionalProperties)} }`;
  return type ?? 'any';
}

const tidy = (text: string | undefined) => text?.replace(/\s*\n\s*/g, ' ').trim();

function fields(root: SchemaNode, node: SchemaNode, skipTag = false) {
  return Object.entries(node.properties ?? {})
    .filter(([name]) => !(skipTag && name === 'type'))
    .map(([name, child]) => ({
      name,
      type: typeText(root, child),
      ...('default' in child ? { default: child.default } : {}),
      doc: tidy(child.description ?? deref(root, child)?.description),
    }));
}

/** Field names, types, defaults and documentation for one schema type. Null when there is no such type. */
export function describeType(root: SchemaNode, name: string): Record<string, unknown> | null {
  const node = root.$defs?.[name];
  if (!node) return null;
  const members = node.oneOf ?? node.anyOf;
  if (members) {
    const tagged = members.map((m) => deref(root, m) ?? m);
    if (tagged.every((m) => m.const !== undefined || m.enum)) {
      return { name, doc: tidy(node.description), kind: 'enum', values: tagged.map((m) => ({ value: m.const ?? m.enum?.[0], doc: tidy(m.description) })) };
    }
    return {
      name,
      doc: tidy(node.description),
      kind: 'tagged union: an object whose "type" selects the variant',
      variants: tagged.map((m) => ({ type: m.properties?.type?.const ?? m.const, doc: tidy(m.description), fields: fields(root, m, true) })),
    };
  }
  if (node.enum) return { name, doc: tidy(node.description), kind: 'enum', values: node.enum };
  return { name, doc: tidy(node.description), kind: 'object', fields: fields(root, node) };
}

/** The top-level profile fields and the type each one holds. */
export function describeRoot(root: SchemaNode): Record<string, unknown> {
  return { name: 'Profile', doc: tidy(root.description), kind: 'object', fields: fields(root, root) };
}

export function typeNames(root: SchemaNode): string[] {
  return Object.keys(root.$defs ?? {});
}

/** Names of the types a type refers to, so a section can be fetched with what it embeds. */
export function referencedTypes(root: SchemaNode, name: string): string[] {
  const found = new Set<string>();
  const visit = (node: SchemaNode | undefined, depth: number) => {
    if (!node || depth > 6) return;
    if (node.$ref) found.add(refName(node.$ref));
    for (const child of Object.values(node.properties ?? {})) visit(child, depth + 1);
    for (const child of node.oneOf ?? []) visit(child, depth + 1);
    for (const child of node.anyOf ?? []) visit(child, depth + 1);
    visit(node.items, depth + 1);
    if (typeof node.additionalProperties === 'object') visit(node.additionalProperties, depth + 1);
  };
  visit(root.$defs?.[name], 0);
  found.delete(name);
  return [...found];
}

/** Findings present after a change that were not there before. */
export function newIssues(before: Issue[], after: Issue[]): Issue[] {
  const seen = new Set(before.map((i) => `${i.code}|${i.path}`));
  return after.filter((i) => !seen.has(`${i.code}|${i.path}`));
}

/** "code at path: message", the form the engine's findings take inside a sentence. */
export const issueText = (i: Issue): string => `${i.code} at ${i.path || '/'}: ${i.message}`;

// ---- scenarios ------------------------------------------------------------------------

/** A short, lowercase id made from a name, unique among `taken` (web/src/state/pointer.ts `uniqueId`). */
export function uniqueId(name: string, taken: Iterable<string | undefined>): string {
  const base =
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '_')
      .replace(/^_+|_+$/g, '')
      .slice(0, 32) || 'item';
  const used = new Set(taken);
  if (!used.has(base)) return base;
  for (let n = 2; ; n++) if (!used.has(`${base}_${n}`)) return `${base}_${n}`;
}

/**
 * Drops a `set` that a later `set` on the same path overrides, looking back only as far
 * as the previous structural edit, so the patch keeps its meaning
 * (web/src/state/patches.ts `compactPatch`).
 */
export function compactPatch(ops: readonly PatchOp[]): PatchOp[] {
  const out: PatchOp[] = [];
  for (const op of ops) {
    if (op.op === 'set') {
      for (let i = out.length - 1; i >= 0; i--) {
        const earlier = out[i] as PatchOp;
        if (earlier.op !== 'set') break;
        if (earlier.path === op.path) {
          out.splice(i, 1);
          break;
        }
      }
    }
    out.push(op);
  }
  return out;
}
