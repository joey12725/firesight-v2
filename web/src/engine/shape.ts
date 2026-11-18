// A very small runtime shape checker (in the spirit of zod, without the dependency).
// A `Shape<T>` checks an unknown value and collects every mismatch with its JSON path.
// Objects are strict: a missing key and an unexpected key are both reported, so a
// field added on the Rust side shows up in the tests instead of going unnoticed.

export interface Shape<T> {
  check(value: unknown, path: string, issues: string[]): void;
  /** Type carrier only. */
  readonly _type?: T;
}

export type Infer<S> = S extends Shape<infer T> ? T : never;

const MAX_ISSUES = 40;

function shape<T>(check: Shape<T>['check']): Shape<T> {
  return {
    check(value, path, issues) {
      if (issues.length < MAX_ISSUES) check(value, path, issues);
    },
  };
}

const describe = (v: unknown) => (v === null ? 'null' : Array.isArray(v) ? 'array' : typeof v === 'number' && !Number.isFinite(v) ? String(v) : typeof v);

export const num: Shape<number> = shape((v, p, issues) => {
  if (typeof v !== 'number' || !Number.isFinite(v)) issues.push(`${p}: expected a finite number, got ${describe(v)}`);
});
export const str: Shape<string> = shape((v, p, issues) => {
  if (typeof v !== 'string') issues.push(`${p}: expected a string, got ${describe(v)}`);
});
export const bool: Shape<boolean> = shape((v, p, issues) => {
  if (typeof v !== 'boolean') issues.push(`${p}: expected a boolean, got ${describe(v)}`);
});
export const anything: Shape<unknown> = shape(() => {});

export function nullable<T>(inner: Shape<T>): Shape<T | null> {
  return shape((v, p, issues) => {
    if (v !== null) inner.check(v, p, issues);
  });
}

export function arr<T>(inner: Shape<T>, length?: number): Shape<T[]> {
  return shape((v, p, issues) => {
    if (!Array.isArray(v)) return void issues.push(`${p}: expected an array, got ${describe(v)}`);
    if (length !== undefined && v.length !== length) issues.push(`${p}: expected ${length} entries, got ${v.length}`);
    for (let i = 0; i < v.length; i++) inner.check(v[i], `${p}[${i}]`, issues);
  });
}

/** One of a fixed list of strings (an enum). */
export function oneOf<const T extends readonly string[]>(values: T): Shape<T[number]> {
  const set = new Set<string>(values);
  return shape((v, p, issues) => {
    if (typeof v !== 'string' || !set.has(v)) issues.push(`${p}: unexpected value ${JSON.stringify(v)}`);
  });
}

export function record<T>(inner: Shape<T>): Shape<Record<string, T>> {
  return shape((v, p, issues) => {
    if (typeof v !== 'object' || v === null || Array.isArray(v)) return void issues.push(`${p}: expected an object, got ${describe(v)}`);
    for (const [k, x] of Object.entries(v)) inner.check(x, `${p}.${k}`, issues);
  });
}

export function obj<F extends Record<string, Shape<unknown>>>(fields: F): Shape<{ [K in keyof F]: Infer<F[K]> }> {
  const keys = Object.keys(fields);
  return shape((v, p, issues) => {
    if (typeof v !== 'object' || v === null || Array.isArray(v)) return void issues.push(`${p}: expected an object, got ${describe(v)}`);
    const o = v as Record<string, unknown>;
    for (const k of keys) {
      if (!(k in o)) issues.push(`${p}.${k}: missing`);
      else (fields[k] as Shape<unknown>).check(o[k], `${p}.${k}`, issues);
    }
    for (const k of Object.keys(o)) if (!(k in fields)) issues.push(`${p}.${k}: not in the TypeScript type (new engine field?)`);
  });
}

/** Every mismatch between `value` and `s`, as "path: problem" lines. Empty means it conforms. */
export function shapeIssues<T>(s: Shape<T>, value: unknown, root = '$'): string[] {
  const issues: string[] = [];
  s.check(value, root, issues);
  return issues;
}

/** Compile-time check that two types are mutually assignable: `const _: Same<A, B> = true`. */
export type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;
