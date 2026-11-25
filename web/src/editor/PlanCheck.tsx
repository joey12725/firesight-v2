import { useState } from 'react';
import { useUi } from '@/state/ui-store';
import { Icon, cx } from '@/ui';
import { useAllIssues, type PlacedIssue } from './hooks';
import { editorMessage } from './issue-words';

/** The editor section an issue path belongs to. */
export function sectionForPath(path: string): string {
  const head = path.split('/')[1] ?? '';
  if (/\/social_security/.test(path)) return 'income';
  const map: Record<string, string> = {
    name: 'household',
    household: 'household',
    incomes: 'income',
    private_equity: 'equity',
    spending: 'spending',
    accounts: 'accounts',
    portfolio: 'accounts',
    properties: 'property',
    debts: 'property',
    debt_policy: 'property',
    events: 'events',
    retirement: 'retirement',
    assumptions: 'assumptions',
    simulation: 'assumptions',
  };
  return map[head] ?? 'household';
}

function Row({ issue }: { issue: PlacedIssue }) {
  const setFocusPath = useUi((s) => s.setFocusPath);
  const error = issue.severity === 'error';
  return (
    <li>
      <button
        type="button"
        onClick={() => setFocusPath(issue.path)}
        className="flex w-full items-start gap-2 rounded-sm px-2 py-1.5 text-left hover:bg-surface-3"
      >
        <Icon name={error ? 'alertCircle' : 'alertTriangle'} size={13} className={cx('mt-0.5', error ? 'text-bad' : 'text-warn')} />
        <span className="min-w-0 flex-1 text-xs leading-snug text-ink-1">
          <span className="sr-only">{error ? 'Error: ' : 'Warning: '}</span>
          {editorMessage(issue)}
        </span>
        <Icon name="arrowRight" size={12} className="mt-0.5 text-ink-3" />
      </button>
    </li>
  );
}

/** Every error and warning from the engine's report, each linking to its field. */
export function PlanCheck() {
  const issues = useAllIssues();
  const [open, setOpen] = useState(true);
  const errors = issues.filter((i) => i.severity === 'error');
  const warnings = issues.filter((i) => i.severity === 'warning');
  if (issues.length === 0) {
    return (
      <div className="flex items-center gap-2 border-b border-line px-4 py-2.5 text-xs text-ink-3">
        <Icon name="checkCircle" size={13} className="text-good" />
        Plan check: nothing to fix
      </div>
    );
  }
  return (
    <div className="border-b border-line">
      <button type="button" aria-expanded={open} onClick={() => setOpen(!open)} className="flex w-full items-center gap-2 px-4 py-2.5 text-left hover:bg-surface-3/50 focus-visible:outline-offset-[-2px]">
        <Icon name={errors.length ? 'alertCircle' : 'alertTriangle'} size={13} className={errors.length ? 'text-bad' : 'text-warn'} />
        <span className="flex-1 text-xs font-medium text-ink-1">
          Plan check
          <span className="ml-2 font-normal text-ink-3">
            {[errors.length && `${errors.length} to fix`, warnings.length && `${warnings.length} to review`].filter(Boolean).join(' · ')}
          </span>
        </span>
        <Icon name="chevronDown" size={14} className={cx('text-ink-3 transition-transform duration-[var(--duration-fast)]', open && 'rotate-180')} />
      </button>
      {open && (
        <ul className="scroll-thin max-h-56 overflow-y-auto px-2 pb-2">
          {[...errors, ...warnings].map((issue, i) => (
            <Row key={`${issue.code}-${issue.path}-${i}`} issue={issue} />
          ))}
        </ul>
      )}
    </div>
  );
}
