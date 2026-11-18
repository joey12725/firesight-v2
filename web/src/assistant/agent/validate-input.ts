// A small JSON Schema checker for tool inputs. Tool inputs are streamed eagerly, so the
// API does not validate them: a truncated or malformed input can arrive as a plausible
// partial object, and must be checked before a tool runs. Covers the keywords the tool
// schemas in this app use (type, enum, required, properties, additionalProperties,
// items, minItems, maxItems, minimum, maximum, minLength, maxLength, anyOf).

export interface JsonSchema {
  type?: string | string[];
  description?: string;
  enum?: readonly unknown[];
  properties?: Record<string, JsonSchema>;
  required?: readonly string[];
  additionalProperties?: boolean;
  items?: JsonSchema;
  minItems?: number;
  maxItems?: number;
  minimum?: number;
  maximum?: number;
  minLength?: number;
  maxLength?: number;
  anyOf?: readonly JsonSchema[];
}

function typeOf(value: unknown): string {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'array';
  if (typeof value === 'number') return Number.isInteger(value) ? 'integer' : 'number';
  return typeof value;
}

function matchesType(value: unknown, type: string): boolean {
  const actual = typeOf(value);
  return actual === type || (type === 'number' && actual === 'integer');
}

/** Problems with `value` against `schema`, as short sentences naming the path. Empty when valid. */
export function validateInput(schema: JsonSchema, value: unknown, path = 'input'): string[] {
  const errors: string[] = [];
  if (schema.anyOf) {
    const fits = schema.anyOf.some((s) => validateInput(s, value, path).length === 0);
    if (!fits) errors.push(`${path} does not match any allowed form`);
    return errors;
  }
  if (schema.type !== undefined) {
    const types = Array.isArray(schema.type) ? schema.type : [schema.type];
    if (!types.some((t) => matchesType(value, t))) {
      errors.push(`${path} must be ${types.join(' or ')}, got ${typeOf(value)}`);
      return errors;
    }
  }
  if (schema.enum && !schema.enum.includes(value)) {
    errors.push(`${path} must be one of ${schema.enum.map((v) => JSON.stringify(v)).join(', ')}`);
  }
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) errors.push(`${path} must be a finite number`);
    if (schema.minimum !== undefined && value < schema.minimum) errors.push(`${path} must be at least ${schema.minimum}`);
    if (schema.maximum !== undefined && value > schema.maximum) errors.push(`${path} must be at most ${schema.maximum}`);
  }
  if (typeof value === 'string') {
    if (schema.minLength !== undefined && value.length < schema.minLength) errors.push(`${path} must not be empty`);
    if (schema.maxLength !== undefined && value.length > schema.maxLength) errors.push(`${path} must be at most ${schema.maxLength} characters`);
  }
  if (Array.isArray(value)) {
    if (schema.minItems !== undefined && value.length < schema.minItems) errors.push(`${path} needs at least ${schema.minItems} item(s)`);
    if (schema.maxItems !== undefined && value.length > schema.maxItems) errors.push(`${path} allows at most ${schema.maxItems} items`);
    if (schema.items) value.forEach((item, i) => errors.push(...validateInput(schema.items as JsonSchema, item, `${path}[${i}]`)));
  }
  if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
    const record = value as Record<string, unknown>;
    for (const key of schema.required ?? []) {
      if (record[key] === undefined) errors.push(`${path}.${key} is required`);
    }
    for (const [key, child] of Object.entries(record)) {
      const sub = schema.properties?.[key];
      if (sub) errors.push(...validateInput(sub, child, `${path}.${key}`));
      else if (schema.additionalProperties === false) {
        errors.push(`${path}.${key} is not a known field (known: ${Object.keys(schema.properties ?? {}).join(', ') || 'none'})`);
      }
    }
  }
  return errors;
}
