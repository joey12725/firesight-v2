// Renderings of the plan and its schema for the model: short, faithful, and always
// carrying the ids that patches address elements by.
import { annualIncome, budgetTotals } from '@/editor/labels';
import type { Issue, PatchOp, Profile, Report } from '@/engine/types';
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
  preview?: unknown;
}

/** What `get_plan` returns, and what is attached to a user turn when the plan has changed. */
export function planView(input: PlanViewInput) {
  const { profile, report, scenarios, schema } = input;
  const { simulation: _simulation, schema_version: _version, ...rest } = profile;
  const plan = pruneDefaults(schema, { properties: schema.properties }, rest) as Record<string, unknown>;
  const totals = budgetTotals(profile);
  const byKind: Record<string, number> = {};
  for (const account of profile.accounts) byKind[account.kind] = (byKind[account.kind] ?? 0) + account.balance;
  const propertyEquity = profile.properties.reduce((n, p) => n + p.value - (p.mortgage?.balance ?? 0), 0);
  const debt = profile.debts.reduce((n, d) => n + d.balance, 0);
  const empty = profile.household.people.length === 0 && profile.incomes.length === 0 && profile.accounts.length === 0 && profile.spending.items.length === 0;
  return {
    today: input.today,
    empty_plan: empty,
    note: 'Fields not shown hold their schema defaults. Amounts are in today\'s dollars; rates are decimals. Address list elements in patches as @id.',
    plan,
    derived: {
      ages_today: profile.household.people.map((p) => ({ id: p.id, age: ageAt(p.birth, input.today) })),
      annual_income_now: Math.round(annualIncome(profile)),
      annual_budget_now: Math.round(totals.now),
      annual_budget_essential_now: Math.round(totals.essentialNow),
      annual_budget_retired: Math.round(totals.retired),
      accounts_by_kind: byKind,
      property_equity: Math.round(propertyEquity),
      non_mortgage_debt: Math.round(debt),
    },
    ...(input.preview !== undefined && input.preview !== null ? { preview: input.preview } : {}),
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
