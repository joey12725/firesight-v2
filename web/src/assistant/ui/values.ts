// Plain-language rendering of plan values in change cards.
import { money, percent, yearMonth } from '@/ui';

const RATE = /(rate|fraction|ratio|share|creep|volatility|probability|vacancy|adjustment|spread|fee|_prob|hazard|growth|raise|extra|yield)$/;
const PLAIN = /(age|months|years|month|person|owner|paths|seed|beta|corr)$/;

const words = (key: string) => key.replace(/_/g, ' ');

/** One value as a person would write it, guided by the field name it sits under. */
export function formatValue(key: string, value: unknown): string {
  if (value === undefined) return 'not set';
  if (value === null) return 'none';
  if (typeof value === 'boolean') return value ? 'yes' : 'no';
  if (typeof value === 'number') {
    if (/factor$/.test(key)) return `× ${Number(value.toPrecision(4))}`;
    if (RATE.test(key)) return percent(value, 2);
    if (key === 'person' || key === 'owner') return who(value);
    if (PLAIN.test(key)) return String(Number(value.toPrecision(6)));
    if (Math.abs(value) >= 1000) return money(Math.round(value));
    return String(Number(value.toPrecision(6)));
  }
  if (typeof value === 'string') return value === '' ? 'empty' : words(value);
  if (Array.isArray(value)) return value.length === 0 ? 'none' : `${value.length} item${value.length === 1 ? '' : 's'}`;
  const when = formatWhen(value as Record<string, unknown>);
  if (when) return when;
  const pairs = describeObject(value as Record<string, unknown>, 3);
  const tag = (value as { type?: unknown }).type;
  const head = typeof tag === 'string' ? words(tag) : '';
  const rest = pairs.map(([k, v]) => `${k} ${v}`).join(', ');
  return [head, rest].filter(Boolean).join(': ') || 'set';
}

const who = (index: unknown) => (index === 1 ? 'second person' : 'first person');

/** A point in time ("age 45", "Jun 2030", "2 years after retirement"), or null when the object is not one. */
function formatWhen(value: Record<string, unknown>): string | null {
  switch (value.type) {
    case 'age':
      return typeof value.age === 'number' ? `age ${value.age}${value.person === 1 ? ' (second person)' : ''}` : null;
    case 'date':
      return typeof value.at === 'string' ? yearMonth(value.at) : null;
    case 'start':
      return Object.keys(value).length === 1 ? 'now' : null;
    case 'retirement': {
      const offset = typeof value.offset_years === 'number' ? value.offset_years : 0;
      return offset === 0 ? 'at retirement' : `${Math.abs(offset)} year${Math.abs(offset) === 1 ? '' : 's'} ${offset > 0 ? 'after' : 'before'} retirement`;
    }
    default:
      return null;
  }
}

/** The telling fields of an object (a new income, an event), flattened one level. */
export function describeObject(value: Record<string, unknown>, limit = 8): [string, string][] {
  const out: [string, string][] = [];
  const visit = (node: Record<string, unknown>, prefix: string, depth: number) => {
    for (const [key, child] of Object.entries(node)) {
      if (out.length >= limit) return;
      if (key === 'id' || key === 'name' || (key === 'type' && depth > 0)) continue;
      if (child === null || child === '' || (Array.isArray(child) && child.length === 0)) continue;
      const label = prefix ? `${prefix} ${words(key)}` : words(key);
      if ((key === 'person' || key === 'owner') && typeof child === 'number') {
        out.push([label, who(child)]);
        continue;
      }
      const when = typeof child === 'object' && !Array.isArray(child) ? formatWhen(child as Record<string, unknown>) : null;
      if (when) {
        out.push([label, when]);
        continue;
      }
      if (typeof child === 'object' && !Array.isArray(child) && depth < 2) {
        const tag = (child as { type?: unknown }).type;
        const inner = Object.keys(child as object).filter((k) => k !== 'type');
        if (typeof tag === 'string' && inner.length === 0) out.push([label, words(tag)]);
        else {
          if (typeof tag === 'string') out.push([label, words(tag)]);
          visit(child as Record<string, unknown>, key === 'kind' ? '' : words(key), depth + 1);
        }
      } else out.push([label, formatValue(key, child)]);
    }
  };
  visit(value, '', 0);
  return out;
}

export const lastSegment = (path: string) => path.split('/').pop() ?? '';
