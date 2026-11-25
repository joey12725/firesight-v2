// Renderings of the plan and its schema for the model: short, faithful, and always
// carrying the ids that patches address elements by.
import { exerciseCostVested, paperValue } from '@/engine/equity-math';
import type { EquityPreview } from '@/engine/equity-types';
import type { PlanPreview } from '@/engine/output-types';
import type { Issue, OutcomeRow, PatchOp, PrivateCompany, Profile, Report } from '@/engine/types';
import type { Scenario } from '@/state/profile-store';
import { canonicalPath, getAt, parsePointer } from '@/state/pointer';
import type { ChangeEntry } from './types';

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

function ageAt(birth: string, today: string): number | null {
  const [by, bm] = birth.split('-').map(Number);
  const [ty, tm] = today.split('-').map(Number);
  if (!by || !bm || !ty || !tm) return null;
  return Math.floor((ty * 12 + tm - (by * 12 + bm)) / 12);
}

const issue = (i: Issue) => ({ code: i.code, path: i.path, message: i.message });

export interface PlanViewInput {
  profile: Profile;
  report: Report;
  scenarios: Scenario[];
  run: { paths: number; seed: number };
  schema: SchemaNode;
  today: string;
  /** Core's plan preview; null when the plan cannot be compiled. */
  preview: PlanPreview | null;
  /** Core's preview of one private company. Left out, or throwing, the odds are omitted and only the paper figures are shown. */
  equityPreview?: (company: PrivateCompany) => EquityPreview;
}

const whole = (v: number) => Math.round(v);
const tenth = (v: number) => Math.round(v * 10) / 10;

/** Core's plan preview as the model reads it: whole dollars, rates to three places. */
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

const ODDS_KINDS = ['fails', 'acquired', 'ipo', 'still_private'] as const;
export type OddsOf100 = Record<(typeof ODDS_KINDS)[number], number>;

/**
 * An outcome table as futures of 100 by kind, to one decimal. The four figures add up
 * to exactly 100: the tenths left over by rounding down go to the largest remainders.
 */
export function oddsOf100(rows: OutcomeRow[]): OddsOf100 {
  const sums = ODDS_KINDS.map((kind) => rows.reduce((n, row) => (row.kind === kind ? n + row.probability : n), 0));
  const total = sums.reduce((a, b) => a + b, 0);
  const tenths = sums.map((p) => (total > 0 ? (p / total) * 1000 : 0));
  const floors = tenths.map(Math.floor);
  let left = total > 0 ? 1000 - floors.reduce((a, b) => a + b, 0) : 0;
  const order = tenths.map((t, i) => ({ i, rest: t - Math.floor(t) })).sort((a, b) => b.rest - a.rest || a.i - b.i);
  for (const { i } of order) {
    if (left <= 0) break;
    floors[i] = (floors[i] as number) + 1;
    left -= 1;
  }
  return Object.fromEntries(ODDS_KINDS.map((kind, i) => [kind, (floors[i] as number) / 10])) as OddsOf100;
}

const places = (v: number, dp: number) => Math.round(v * 10 ** dp) / 10 ** dp;
const price = (v: number) => Number(v.toPrecision(4));

/**
 * What the plan implies about one private company before any simulation: the paper
 * figures, and the odds the model will use. Paper value is at the latest preferred
 * price and is in no balance. Null for a company with no price yet.
 */
function companyView(company: PrivateCompany, otherNetWorth: number, equityPreview: PlanViewInput['equityPreview']) {
  if (!(company.preferred_price > 0)) return null;
  const paper = paperValue(company);
  const denominator = otherNetWorth + paper.total;
  const base = {
    id: company.id,
    name: company.name,
    included: company.included,
    paper_value_vested: whole(paper.vested),
    paper_value_total: whole(paper.total),
    exercise_cost_vested: whole(exerciseCostVested(company)),
    share_of_net_worth: denominator > 0 ? places(paper.total / denominator, 3) : null,
  };
  let p: EquityPreview;
  try {
    if (!equityPreview) return base;
    p = equityPreview(company);
  } catch {
    return base;
  }
  return {
    ...base,
    odds_in_force: oddsOf100(p.table),
    typical_odds: oddsOf100(p.typical),
    typical_rows: typicalRows(p.typical),
    implied_multiple: places(p.implied_multiple, 3),
    typical_implied_multiple: places(p.typical_implied_multiple, 3),
    nothing_share: places(p.nothing_share, 3),
    tender_before_share: places(p.tender_before_share, 3),
    spread_iso_vested: whole(p.spread_iso_vested),
    spread_nso_vested: whole(p.spread_nso_vested),
    fmv_409a: price(p.fmv_409a),
    fmv_fraction: places(p.fmv_409a / company.preferred_price, 3),
    fmv_409a_basis: company.fmv_409a === null ? 'typical for the stage' : 'stated in the plan',
    tender_annual_probability: places(p.tender_annual_probability, 3),
  };
}

/**
 * The stage's typical table as rows a custom `outcomes` table can be built from: each
 * row's odds with its timing and its price range, rounded as the editor seeds them
 * (years to the half year, multiples to two significant figures). A custom table
 * inherits nothing from the stage, so these are what a row must carry besides its
 * probability.
 */
export function typicalRows(rows: OutcomeRow[]) {
  const half = (v: number) => Math.round(v * 2) / 2;
  const sig2 = (v: number) => Number(v.toPrecision(2));
  return rows.map((row) => {
    const from = Math.min(30, Math.max(0, half(row.years_from)));
    const low = sig2(row.multiple_low);
    return {
      id: row.kind,
      kind: row.kind,
      probability: places(row.probability, 4),
      years_from: from,
      years_to: Math.min(30, Math.max(from, half(row.years_to))),
      multiple_low: low,
      multiple_high: Math.max(low, sig2(row.multiple_high)),
    };
  });
}

const EQUITY_NOTE =
  'Paper value is at the latest preferred price, options at price less strike, before tax; it is in no balance above and in no net worth figure. odds_in_force and typical_odds are futures of 100 over the next ten years (a company still private then faces the same odds again); typical_rows is the stage\'s typical table with each row\'s timing (years from today) and price range (multiples of the last round\'s price for an IPO, of the last valuation for a sale), ready to copy into a custom outcomes table; implied_multiple is the mean common-stock price at the outcome over the preferred price, counting futures that pay nothing as 0; nothing_share and tender_before_share are shares of futures over every round. These are the model\'s inputs, not results of a run.';

/** What `get_plan` returns, and what is attached to a user turn when the plan has changed. */
export function planView(input: PlanViewInput) {
  const { profile, report, scenarios, schema } = input;
  const { simulation: _simulation, schema_version: _version, ...rest } = profile;
  const plan = pruneDefaults(schema, { properties: schema.properties }, rest) as Record<string, unknown>;
  const byKind: Record<string, number> = {};
  for (const account of profile.accounts) byKind[account.kind] = (byKind[account.kind] ?? 0) + account.balance;
  const propertyEquity = profile.properties.reduce((n, p) => n + p.value - (p.mortgage?.balance ?? 0), 0);
  const debt = profile.debts.reduce((n, d) => n + d.balance, 0);
  const balances = profile.accounts.reduce((n, a) => n + a.balance, 0);
  const companies = (profile.private_equity ?? []).flatMap((company) => companyView(company, balances + propertyEquity - debt, input.equityPreview) ?? []);
  const empty = profile.household.people.length === 0 && profile.incomes.length === 0 && profile.accounts.length === 0 && profile.spending.items.length === 0;
  return {
    today: input.today,
    empty_plan: empty,
    note: 'Fields not shown hold their schema defaults. Amounts are in today\'s dollars; rates are decimals. Address list elements in patches as @id.',
    plan,
    derived: {
      ages_today: profile.household.people.map((p) => ({ id: p.id, age: ageAt(p.birth, input.today) })),
      accounts_by_kind: byKind,
      property_equity: Math.round(propertyEquity),
      non_mortgage_debt: Math.round(debt),
      ...(companies.length > 0 ? { private_equity: companies, private_equity_note: EQUITY_NOTE } : {}),
    },
    ...(input.preview ? { preview: previewView(input.preview) } : {}),
    validation: {
      can_run: report.errors.length === 0,
      errors: report.errors.slice(0, 20).map(issue),
      warnings: report.warnings.slice(0, 20).map(issue),
      ...(report.errors.length + report.warnings.length > 40 ? { truncated: true } : {}),
    },
    scenarios: scenarios.map((s) => ({ id: s.id, name: s.name, enabled: s.enabled, ops: s.patch.length <= 10 ? s.patch : { count: s.patch.length, first: s.patch.slice(0, 10) } })),
    run_settings: { paths: input.run.paths, seed: input.run.seed, end_age: profile.simulation.end_age },
  };
}

// ---- schema lookups -----------------------------------------------------------------

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

/** Field names, types, defaults and documentation for one schema type. */
export function describeType(root: SchemaNode, name: string): unknown {
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
export function describeRoot(root: SchemaNode): unknown {
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

// ---- diffs --------------------------------------------------------------------------

const words = (segment: string) => segment.replace(/_/g, ' ');

/** A patch path in words, with `@id` segments replaced by the element's name. */
export function pathLabel(profile: unknown, path: string): string {
  const segments = parsePointer(path);
  const out: string[] = [];
  let pointer = '';
  for (const segment of segments) {
    pointer += `/${segment}`;
    if (segment === '-') continue;
    if (segment.startsWith('@') || /^\d+$/.test(segment)) {
      const el = getAt(profile, pointer) as { name?: string; id?: string } | undefined;
      out.push(el?.name || el?.id || segment.replace(/^@/, ''));
    } else if (segment !== 'kind') out.push(words(segment));
  }
  return out.join(' / ') || 'plan';
}

/** One entry per operation: where it lands, what is there now, what it becomes. */
export function diffEntries(profile: Profile, ops: PatchOp[]): ChangeEntry[] {
  return ops.map((op) => {
    const path = canonicalPath(profile, op.path);
    const label = pathLabel(profile, op.path);
    if (op.op === 'remove') return { op: op.op, path, label, before: getAt(profile, op.path), after: undefined };
    if (op.op === 'append') {
      const name = (op.value as { name?: string; id?: string } | null)?.name ?? (op.value as { id?: string } | null)?.id;
      return { op: op.op, path, label: name ? `${label} / ${name}` : label, before: undefined, after: op.value };
    }
    return { op: op.op, path, label, before: getAt(profile, op.path), after: op.value };
  });
}

/** Findings present after a change that were not there before, as "code at path: message". */
export function newIssues(before: Issue[], after: Issue[]): string[] {
  const seen = new Set(before.map((i) => `${i.code}|${i.path}`));
  return after.filter((i) => !seen.has(`${i.code}|${i.path}`)).map((i) => `${i.code} at ${i.path || '/'}: ${i.message}`);
}
