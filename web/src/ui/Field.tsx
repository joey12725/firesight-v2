import { useId, type ReactNode } from 'react';
import { cx } from './cx';
import { Icon } from './icons';
import { Popover } from './Popover';

export interface FieldIssue {
  severity: 'error' | 'warning';
  message: string;
}

export interface FieldProps {
  label: ReactNode;
  /** Longer explanation, shown in a popover from the help button. */
  help?: ReactNode;
  /** One short line under the control. */
  hint?: ReactNode;
  issues?: FieldIssue[];
  /** Label beside the control instead of above it (switches, compact rows). */
  inline?: boolean;
  className?: string;
  /** Receives the ids to wire onto the control. */
  children: (ids: { id: string; describedBy: string | undefined; invalid: boolean }) => ReactNode;
}

export function HelpButton({ label, children }: { label: string; children: ReactNode }) {
  return (
    <Popover
      label={`About ${label}`}
      placement="bottom-start"
      className="w-72 p-3 text-sm leading-relaxed text-ink-2"
      trigger={
        <button
          type="button"
          aria-label={`About ${label}`}
          className="inline-flex size-4 items-center justify-center rounded-full text-ink-3 hover:text-ink-1"
        >
          <Icon name="help" size={13} />
        </button>
      }
    >
      {children}
    </Popover>
  );
}

export function IssueList({ issues, id }: { issues: FieldIssue[]; id?: string }) {
  if (issues.length === 0) return null;
  return (
    <ul id={id} className="mt-1 space-y-0.5">
      {issues.map((issue, i) => (
        <li
          key={i}
          className={cx('flex items-start gap-1.5 text-xs leading-snug', issue.severity === 'error' ? 'text-bad' : 'text-warn')}
        >
          <Icon name={issue.severity === 'error' ? 'alertCircle' : 'alertTriangle'} size={12} className="mt-[2px]" />
          <span>
            <span className="sr-only">{issue.severity === 'error' ? 'Error: ' : 'Warning: '}</span>
            {issue.message}
          </span>
        </li>
      ))}
    </ul>
  );
}

/** Label, help, control and inline validation for one value. */
export function Field({ label, help, hint, issues = [], inline, className, children }: FieldProps) {
  const id = useId();
  const noteId = `${id}-note`;
  const hasNote = issues.length > 0 || Boolean(hint);
  const invalid = issues.some((i) => i.severity === 'error');
  const labelText = typeof label === 'string' ? label : 'this field';
  const control = children({ id, describedBy: hasNote ? noteId : undefined, invalid });

  const labelRow = (
    <span className="flex min-w-0 items-center gap-1">
      <label htmlFor={id} className="truncate text-xs font-medium text-ink-2">
        {label}
      </label>
      {help && <HelpButton label={labelText}>{help}</HelpButton>}
    </span>
  );

  return (
    <div className={cx('min-w-0', className)}>
      {inline ? (
        <div className="flex min-h-8 items-center justify-between gap-3">
          {labelRow}
          <span className="shrink-0">{control}</span>
        </div>
      ) : (
        <>
          <div className="mb-1">{labelRow}</div>
          {control}
        </>
      )}
      {hasNote && (
        <div id={noteId}>
          {hint && issues.length === 0 && <p className="mt-1 text-xs leading-snug text-ink-3">{hint}</p>}
          <IssueList issues={issues} />
        </div>
      )}
    </div>
  );
}
