// Which life to read: the representative lives picked by a stated rule, and a way to
// walk through the others one at a time, at the reader's pace. Nothing plays by itself.
import { useState } from 'react';
import { Button, IconButton, TextInput, cx } from '@/ui';
import type { LifeChoice } from './life';

export interface PickerProps {
  choices: LifeChoice[];
  path: number;
  /** Lives in the run: path numbers run from 0 to one less. */
  paths: number;
  onPick: (pathIndex: number, why: LifeChoice['why'] | 'sample') => void;
}

export function Picker({ choices, path, paths, onPick }: PickerProps) {
  const [draft, setDraft] = useState({ text: String(path), path });
  if (draft.path !== path) setDraft({ text: String(path), path });
  const last = Math.max(0, paths - 1);
  const go = () => {
    const n = Number(draft.text.replace(/[^\d]/g, ''));
    if (draft.text.trim() === '' || !Number.isFinite(n)) return setDraft({ text: String(path), path });
    const next = Math.min(last, Math.max(0, Math.round(n)));
    if (next !== path) onPick(next, 'sample');
    else setDraft({ text: String(path), path });
  };
  return (
    <div>
      <ul className="scroll-thin -mx-1 flex snap-x gap-2 overflow-x-auto px-1 pb-2 @3xl:grid @3xl:grid-cols-4 @6xl:grid-cols-7 @3xl:overflow-visible" aria-label="Representative lives">
        {choices.map((c) => {
          const active = c.pathIndex === path;
          return (
            <li key={c.key} className="w-52 shrink-0 snap-start @3xl:w-auto">
              <button
                type="button"
                aria-pressed={active}
                data-testid={`life-${c.key}`}
                onClick={() => onPick(c.pathIndex, c.why)}
                className={cx(
                  'flex h-full w-full flex-col rounded-md border px-3 py-2.5 text-left transition-colors duration-[var(--duration-fast)]',
                  active ? 'border-ink-1 bg-surface-2' : 'border-line hover:border-line-strong hover:bg-surface-1',
                )}
              >
                <span className="text-sm font-medium text-ink-1">{c.title}</span>
                <span className="mt-0.5 text-xs leading-snug text-pretty text-ink-3">{c.rule}</span>
                {c.describes && <span className="mt-1.5 text-xs leading-snug text-pretty text-ink-2">{c.describes}</span>}
                <span className="num mt-auto pt-1.5 text-2xs text-ink-3">Life #{c.pathIndex}</span>
              </button>
            </li>
          );
        })}
      </ul>
      <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2 text-xs text-ink-2">
        <span>Or read any other:</span>
        <div className="flex items-center gap-1">
          <IconButton icon="chevronLeft" size="sm" label="Previous life" disabled={path <= 0} onClick={() => onPick(path - 1, 'sample')} />
          <label className="flex items-center gap-1.5">
            <span>Life #</span>
            <TextInput
              className="num !h-7 w-20 text-xs"
              inputMode="numeric"
              aria-label={`Life number, 0 to ${last}`}
              value={draft.text}
              onChange={(text) => setDraft({ text, path })}
              onBlur={go}
              onKeyDown={(e) => {
                if (e.key === 'Enter') go();
              }}
            />
          </label>
          <IconButton icon="chevronRight" size="sm" label="Next life" disabled={path >= last} onClick={() => onPick(path + 1, 'sample')} />
        </div>
        <Button size="sm" variant="ghost" icon="rotate" onClick={() => onPick(Math.floor(Math.random() * paths), 'sample')}>
          Another life at random
        </Button>
        <span className="text-ink-3">of {paths.toLocaleString('en-US')}, each as likely as the next.</span>
      </div>
    </div>
  );
}
