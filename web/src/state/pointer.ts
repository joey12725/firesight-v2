// Patch-pointer helpers. Paths are JSON pointers in which an array segment may be
// `@<id>` (the element whose `id` matches), exactly as core `patch.rs` reads them.

export function parsePointer(path: string): string[] {
  if (path === '' || path === '/') return [];
  return path
    .replace(/^\//, '')
    .split('/')
    .map((s) => s.replace(/~1/g, '/').replace(/~0/g, '~'));
}

function escape(segment: string | number): string {
  return String(segment).replace(/~/g, '~0').replace(/\//g, '~1');
}

/** Joins segments into a pointer. `join('/incomes', at('a'), 'kind')` gives `/incomes/@a/kind`. */
export function join(base: string, ...segments: (string | number)[]): string {
  return segments.reduce<string>((path, s) => `${path}/${escape(s)}`, base === '/' ? '' : base);
}

/** The segment that addresses a list element: `@id` when it has an id, else its index. */
export function at(element: { id?: string } | undefined, index = 0): string {
  return element?.id ? `@${element.id}` : String(index);
}

function step(node: unknown, segment: string): unknown {
  if (Array.isArray(node)) {
    if (segment.startsWith('@')) {
      const id = segment.slice(1);
      return node.find((el) => (el as { id?: unknown } | null)?.id === id);
    }
    return node[Number(segment)];
  }
  if (node !== null && typeof node === 'object') return (node as Record<string, unknown>)[segment];
  return undefined;
}

/** The value at `path`, or undefined when any segment is missing. */
export function getAt(root: unknown, path: string): unknown {
  let node = root;
  for (const segment of parsePointer(path)) {
    if (node === undefined || node === null) return undefined;
    node = step(node, segment);
  }
  return node;
}

/**
 * Rewrites numeric array segments as `@id` where the element has one, so paths from
 * different sources (validator, editor, levers) compare equal.
 */
export function canonicalPath(root: unknown, path: string): string {
  let node = root;
  let out = '';
  for (const segment of parsePointer(path)) {
    let canonical = segment;
    if (Array.isArray(node) && /^\d+$/.test(segment)) {
      const id = (node[Number(segment)] as { id?: unknown } | undefined)?.id;
      if (typeof id === 'string' && id !== '') canonical = `@${id}`;
    }
    node = node === undefined || node === null ? undefined : step(node, segment);
    out += `/${escape(canonical)}`;
  }
  return out;
}

/** True when `path` is `prefix` or lies beneath it. */
export function isWithin(path: string, prefix: string): boolean {
  return path === prefix || path.startsWith(`${prefix}/`);
}

/** A slug that is unique among `taken`, for new list elements. */
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
