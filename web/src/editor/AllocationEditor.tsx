import { useRef } from 'react';
import type { Allocation, AssetClass } from '@/engine/types';
import { set } from '@/state/patches';
import { IconButton, IssueList, NumberInput, Select, cx } from '@/ui';
import { equityShare, setWeight, shiftBoundary, toAllocation, toPercents, type Weights } from './allocation';
import { useEdit, useIssues, useValue } from './hooks';
import { ASSETS } from './labels';

const asset = (key: AssetClass) => ASSETS.find((a) => a.value === key) as (typeof ASSETS)[number];

/**
 * The target mix as one stacked bar. Drag a boundary (or focus it and use the arrow
 * keys) to move weight between neighbours; type a figure to set one asset and scale
 * the rest. The total is always 100%.
 */
export function AllocationEditor({ path, label }: { path: string; label: string }) {
  const allocation = useValue<Allocation | null>(path) ?? {};
  const edit = useEdit();
  const issues = useIssues(path, true);
  const bar = useRef<HTMLDivElement>(null);
  const weights = toPercents(allocation);
  const write = (next: Weights) => edit(set(path, toAllocation(next)), { coalesce: path, label: `Change ${label.toLowerCase()}` });
  const unused = ASSETS.filter((a) => !weights.some((w) => w.key === a.value));

  const drag = (index: number) => (down: React.PointerEvent) => {
    const el = bar.current;
    if (!el) return;
    down.preventDefault();
    (down.target as HTMLElement).setPointerCapture(down.pointerId);
    const width = el.getBoundingClientRect().width;
    const startX = down.clientX;
    const start = weights;
    const move = (e: PointerEvent) => write(shiftBoundary(start, index, ((e.clientX - startX) / width) * 100));
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };

  const edges = weights.map((_, i) => weights.slice(0, i + 1).reduce((n, w) => n + w.pct, 0));
  return (
    <div data-path={path} className="space-y-2.5">
      <div className="flex items-baseline justify-between">
        <span className="text-xs font-medium text-ink-2">{label}</span>
        <span className="text-xs text-ink-3">
          <span className="num text-ink-2">{equityShare(weights)}%</span> growth assets
        </span>
      </div>

      <div ref={bar} className="relative flex h-7 overflow-hidden rounded-sm bg-surface-3" role="group" aria-label={`${label}: drag the boundaries to adjust`}>
        {weights.map((w) => (
          <div
            key={w.key}
            style={{ width: `${w.pct}%`, background: asset(w.key).color }}
            className="flex min-w-0 items-center justify-center overflow-hidden text-2xs font-medium text-white/95 transition-[width] duration-[var(--duration-fast)]"
            title={`${asset(w.key).label}: ${w.pct}%`}
          >
            {w.pct >= 12 && <span className="truncate px-1">{asset(w.key).short}</span>}
          </div>
        ))}
        {weights.slice(0, -1).map((w, i) => {
          const next = weights[i + 1] as Weights[number];
          return (
            <div
              key={w.key}
              role="slider"
              tabIndex={0}
              aria-label={`Boundary between ${asset(w.key).label} and ${asset(next.key).label}`}
              aria-valuemin={0}
              aria-valuemax={w.pct + next.pct}
              aria-valuenow={w.pct}
              aria-valuetext={`${asset(w.key).label} ${w.pct}%, ${asset(next.key).label} ${next.pct}%`}
              style={{ left: `${edges[i]}%` }}
              onPointerDown={drag(i)}
              onKeyDown={(e) => {
                const step = e.shiftKey ? 5 : 1;
                if (e.key === 'ArrowRight' || e.key === 'ArrowUp') write(shiftBoundary(weights, i, step));
                else if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') write(shiftBoundary(weights, i, -step));
                else return;
                e.preventDefault();
              }}
              className="group absolute inset-y-0 z-10 -ml-2 flex w-4 cursor-ew-resize touch-none justify-center focus-visible:outline-offset-[-2px]"
            >
              <span className="h-full w-0.5 bg-surface-1 group-hover:w-1 group-focus-visible:w-1" />
            </div>
          );
        })}
      </div>

      <ul className="space-y-1">
        {weights.map((w) => (
          <li key={w.key} className="flex items-center gap-2">
            <span aria-hidden className="size-2 shrink-0 rounded-[2px]" style={{ background: asset(w.key).color }} />
            <span className="min-w-0 flex-1 truncate text-sm text-ink-1">{asset(w.key).label}</span>
            <NumberInput
              aria-label={`${asset(w.key).label} percent`}
              className="w-[4.5rem]"
              kind="integer"
              suffix="%"
              min={0}
              max={100}
              value={w.pct}
              onChange={(v) => v !== null && v >= 0 && v <= 100 && write(setWeight(weights, w.key, v))}
            />
            <IconButton
              icon="x"
              size="sm"
              label={`Remove ${asset(w.key).label}`}
              disabled={weights.length <= 1}
              className={cx(weights.length <= 1 && 'invisible')}
              onClick={() => write(setWeight(weights, w.key, 0))}
            />
          </li>
        ))}
      </ul>

      {unused.length > 0 && (
        <Select
          aria-label="Add an asset class"
          value={'' as AssetClass}
          placeholder="Add an asset class…"
          options={unused.map((a) => ({ value: a.value, label: a.label }))}
          onChange={(key) => write(setWeight(weights, key, weights.length === 0 ? 100 : 5))}
        />
      )}
      <IssueList issues={issues} />
    </div>
  );
}
