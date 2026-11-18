import type { PatchOp } from '@/engine/types';

export const set = (path: string, value: unknown): PatchOp => ({ op: 'set', path, value });
export const remove = (path: string): PatchOp => ({ op: 'remove', path });
export const append = (path: string, value: unknown): PatchOp => ({ op: 'append', path, value });

/**
 * Drops a `set` that a later `set` on the same path overrides. Only looks back as far
 * as the previous structural edit (`append` / `remove`), so the patch keeps its meaning.
 */
export function compactPatch(ops: PatchOp[]): PatchOp[] {
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

/** A short human description of one operation, for scenario diffs and history labels. */
export function describeOp(op: PatchOp): string {
  const where = op.path.replace(/^\//, '').replace(/\/@/g, ' / ').replace(/\//g, ' / ').replace(/_/g, ' ');
  if (op.op === 'remove') return `Remove ${where}`;
  if (op.op === 'append') {
    const name = (op.value as { name?: string } | null)?.name;
    return `Add ${name ? `"${name}" to ` : 'to '}${where}`;
  }
  const v = op.value;
  const shown =
    v === null
      ? 'none'
      : typeof v === 'object'
        ? ((v as { type?: string }).type ?? '…').replace(/_/g, ' ')
        : String(v);
  return `${where} → ${shown}`;
}
