// Help text for editor fields comes from the profile JSON Schema (the doc comments on
// the Rust types). The schema is read from the wasm module, which embeds the same
// document that `npm run schema` writes to profile.schema.json.
import { schema as loadSchema } from '@/engine';
import { parsePointer } from '@/state/pointer';

interface Node {
  $ref?: string;
  description?: string;
  type?: string | string[];
  const?: unknown;
  properties?: Record<string, Node>;
  additionalProperties?: Node | boolean;
  items?: Node;
  oneOf?: Node[];
  anyOf?: Node[];
  $defs?: Record<string, Node>;
}

let root: Node | null = null;
const getRoot = () => (root ??= loadSchema() as Node);

function deref(node: Node | undefined): Node | undefined {
  let n = node;
  while (n?.$ref) n = getRoot().$defs?.[n.$ref.replace('#/$defs/', '')];
  return n;
}

/** The concrete alternatives of a node: itself, or the non-null members of its union. */
function variants(node: Node): Node[] {
  const members = node.oneOf ?? node.anyOf;
  if (!members) return [node];
  return members.flatMap((m) => {
    const d = deref(m);
    return d && d.type !== 'null' ? variants(d) : [];
  });
}

function tidy(text: string | undefined): string | undefined {
  return text?.replace(/\s*\n\s*/g, ' ').trim() || undefined;
}

function walk(node: Node | undefined, value: unknown, segments: string[]): Node | undefined {
  if (!node) return undefined;
  if (segments.length === 0) return node;
  const [segment = '', ...rest] = segments;
  const options = variants(deref(node) ?? {});

  const list = options.find((o) => o.items);
  if (list && (Array.isArray(value) || /^(\d+|@.*|-)$/.test(segment))) {
    const child = Array.isArray(value)
      ? segment.startsWith('@')
        ? value.find((el) => (el as { id?: unknown } | null)?.id === segment.slice(1))
        : value[Number(segment)]
      : undefined;
    return walk(list.items, child, rest);
  }

  const tag = (value as { type?: unknown } | null | undefined)?.type;
  const objects = options.filter((o) => o.properties || o.additionalProperties);
  const chosen =
    objects.find((o) => typeof tag === 'string' && o.properties?.type?.const === tag) ??
    objects.find((o) => o.properties?.[segment]) ??
    objects[0];
  if (!chosen) return undefined;
  const child = (value as Record<string, unknown> | null | undefined)?.[segment];
  const property = chosen.properties?.[segment];
  if (property) return walk(property, child, rest);
  if (typeof chosen.additionalProperties === 'object') return walk(chosen.additionalProperties, child, rest);
  return undefined;
}

/**
 * The schema description of the value at a patch path. `profile` picks the right
 * variant of tagged unions along the way (a job's fields versus a pension's).
 */
export function describe(profile: unknown, path: string): string | undefined {
  const node = walk(getRoot(), profile, parsePointer(path));
  return tidy(node?.description ?? deref(node)?.description);
}

/**
 * Descriptions of each alternative of the union at `path`, keyed by its tag (the
 * `type` of a tagged object, or the string itself for plain enums).
 */
export function describeVariants(profile: unknown, path: string): Record<string, string> {
  const node = deref(walk(getRoot(), profile, parsePointer(path)));
  const out: Record<string, string> = {};
  if (!node) return out;
  for (const v of variants(node)) {
    const key = typeof v.const === 'string' ? v.const : v.properties?.type?.const;
    const text = tidy(v.description);
    if (typeof key === 'string' && text) out[key] = text;
  }
  return out;
}
