// Page furniture shared by the Outcomes and Assumptions views: the skeleton shown before
// the first result, the tag that says how sharp the picture is, and section headings.
import type { ReactNode } from 'react';
import type { Summary } from '@/engine/output-types';
import type { RunState } from '@/state/run-store';
import { useSettings } from '@/state/settings-store';
import { useUi } from '@/state/ui-store';
import { Icon, Skeleton, Tag, count, cx } from '@/ui';

export function ViewSkeleton() {
  return (
    <div className="mx-auto max-w-[76rem] space-y-10 py-8" aria-busy="true" aria-label="Simulating">
      <div className="space-y-3">
        <Skeleton className="h-3 w-28" />
        <Skeleton className="h-8 w-full max-w-2xl" />
        <Skeleton className="h-8 w-2/3 max-w-xl" />
      </div>
      <div className="grid grid-cols-1 gap-8 md:grid-cols-2">
        <Skeleton className="h-56 w-full" />
        <Skeleton className="h-56 w-full" />
      </div>
      <Skeleton className="h-72 w-full" />
    </div>
  );
}

/** How far along the picture is: nothing once it is settled, a quiet tag while it is still coarse. Mirrors the Overview. */
export function Sharpness({ run, summary }: { run: RunState; summary: Summary }) {
  if (run.stale) return <Tag>Updating…</Tag>;
  if (run.status === 'running' && summary.paths < run.total) return <Tag tone={summary.paths < 1000 ? 'warn' : 'neutral'}>{summary.paths < 1000 ? `Early picture: ${count(summary.paths)} lives so far` : `Sharpening: ${count(summary.paths)} lives so far`}</Tag>;
  if (summary.paths < 100)
    return (
      <Tag tone="warn" icon="alertTriangle">
        {`Only ${summary.paths} lives simulated: treat every figure as rough`}
      </Tag>
    );
  if (summary.paths < 1000) return <Tag tone="warn">{`${count(summary.paths)} lives: shares are good to about ±${Math.round(100 / Math.sqrt(summary.paths))} in 100`}</Tag>;
  return null;
}

export function SectionHead({ title, children, aside, className }: { title: string; children?: ReactNode; aside?: ReactNode; className?: string }) {
  return (
    <div className={cx('mt-14 flex flex-wrap items-baseline gap-x-4 gap-y-1 border-t border-line pt-5', className)}>
      <h2 className="text-base font-semibold tracking-tight text-ink-1">{title}</h2>
      {aside && <div className="ml-auto">{aside}</div>}
      {children && <p className="basis-full max-w-[46rem] text-sm leading-relaxed text-pretty text-ink-2">{children}</p>}
    </div>
  );
}

/** Opens the plan editor at the field that holds an assumption (and the editor sheet on narrow screens). */
export function openEditorAt(path: string) {
  const ui = useUi.getState();
  if (!window.matchMedia('(min-width: 768px)').matches) ui.openSheet('editor');
  else if (!useSettings.getState().editorOpen) useSettings.getState().set({ editorOpen: true });
  // After the sheet has mounted, so the editor's focus effect finds the field.
  window.setTimeout(() => useUi.getState().setFocusPath(path), 60);
}

export function EditLink({ path, children }: { path: string; children: ReactNode }) {
  return (
    <button type="button" className="inline-flex items-center gap-1 rounded-xs text-xs font-medium text-accent-ink hover:underline" onClick={() => openEditorAt(path)}>
      {children}
      <Icon name="arrowRight" size={12} />
    </button>
  );
}
