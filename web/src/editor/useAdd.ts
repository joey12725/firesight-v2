import { append } from '@/state/patches';
import { getAt, uniqueId } from '@/state/pointer';
import { selectEffectiveProfile, useProfileStore } from '@/state/profile-store';
import { useEdit } from './hooks';
import { markFresh } from './Section';

/**
 * Appends a new element to the list at `listPath` with a fresh id; the engine fills in
 * every field not given. The new item opens expanded. Pass `unnamed` for element types
 * without a `name` field (the schema rejects unknown fields).
 */
export function useAdd(listPath: string) {
  const edit = useEdit();
  return (name: string, value: Record<string, unknown> = {}, options: { idHint?: string; unnamed?: boolean; taken?: string[] } = {}) => {
    const list = (getAt(selectEffectiveProfile(useProfileStore.getState()), listPath) as { id?: string }[] | undefined) ?? [];
    const id = uniqueId(options.idHint ?? name, [...list.map((x) => x.id), ...(options.taken ?? [])]);
    markFresh(`${listPath}/@${id}`);
    edit(append(listPath, options.unnamed ? { id, ...value } : { id, name, ...value }), { label: `Add ${name}` });
    return id;
  };
}
