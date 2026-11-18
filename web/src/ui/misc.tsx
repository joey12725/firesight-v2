import { useId, useState, type HTMLAttributes, type ReactNode, type TdHTMLAttributes } from 'react';
import { cx } from './cx';
import { Icon, type IconName } from './icons';

// ---- Disclosure -------------------------------------------------------------------

interface DisclosureProps {
  summary: ReactNode;
  children: ReactNode;
  defaultOpen?: boolean;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  className?: string;
}

/** A small show/hide for secondary detail ("Advanced", "More options"). */
export function Disclosure({ summary, children, defaultOpen = false, open: controlled, onOpenChange, className }: DisclosureProps) {
  const [inner, setInner] = useState(defaultOpen);
  const open = controlled ?? inner;
  const id = useId();
  return (
    <div className={className}>
      <button
        type="button"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => {
          setInner(!open);
          onOpenChange?.(!open);
        }}
        className="flex items-center gap-1 rounded-xs text-xs font-medium text-ink-2 hover:text-ink-1"
      >
        <Icon name="chevronRight" size={12} className={cx('transition-transform duration-[var(--duration-fast)]', open && 'rotate-90')} />
        {summary}
      </button>
      {open && (
        <div id={id} className="pt-3">
          {children}
        </div>
      )}
    </div>
  );
}

// ---- Tag --------------------------------------------------------------------------

type Tone = 'neutral' | 'accent' | 'cool' | 'good' | 'warn' | 'bad';

const TAG: Record<Tone, string> = {
  neutral: 'bg-surface-3 text-ink-2',
  accent: 'bg-accent-soft text-accent-ink',
  cool: 'bg-cool-soft text-cool-ink',
  good: 'bg-good-soft text-good',
  warn: 'bg-warn-soft text-warn',
  bad: 'bg-bad-soft text-bad',
};

export function Tag({ tone = 'neutral', icon, children, className }: { tone?: Tone; icon?: IconName; children: ReactNode; className?: string }) {
  return (
    <span className={cx('inline-flex h-5 items-center gap-1 rounded-xs px-1.5 text-2xs font-medium whitespace-nowrap', TAG[tone], className)}>
      {icon && <Icon name={icon} size={11} />}
      {children}
    </span>
  );
}

// ---- EmptyState -------------------------------------------------------------------

interface EmptyStateProps {
  icon?: IconName;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
  tone?: 'neutral' | 'bad' | 'warn';
  /** Left-aligned and compact, for use inside a column. */
  compact?: boolean;
  className?: string;
}

export function EmptyState({ icon, title, children, action, tone = 'neutral', compact, className }: EmptyStateProps) {
  return (
    <div className={cx(compact ? 'py-4' : 'mx-auto flex max-w-md flex-col items-center px-6 py-16 text-center', className)}>
      {icon && (
        <Icon
          name={icon}
          size={compact ? 16 : 20}
          className={cx(compact ? 'mb-2' : 'mb-4', tone === 'bad' ? 'text-bad' : tone === 'warn' ? 'text-warn' : 'text-ink-3')}
        />
      )}
      <h3 className={cx('font-medium text-ink-1', compact ? 'text-sm' : 'text-lg tracking-tight')}>{title}</h3>
      {children && <div className={cx('mt-1.5 text-sm leading-relaxed text-ink-2', !compact && 'text-pretty')}>{children}</div>}
      {action && <div className={cx('flex gap-2', compact ? 'mt-3' : 'mt-5')}>{action}</div>}
    </div>
  );
}

// ---- Skeleton ---------------------------------------------------------------------

/** A placeholder block while content loads. Size it with className (h-*, w-*). */
export function Skeleton({ className }: { className?: string }) {
  return (
    <span
      aria-hidden
      className={cx(
        'block rounded-xs bg-[linear-gradient(90deg,var(--surface-3)_25%,var(--line)_50%,var(--surface-3)_75%)] bg-[length:200%_100%] animate-shimmer',
        className,
      )}
    />
  );
}

// ---- Table ------------------------------------------------------------------------

/** A compact data table: hairline rows, eyebrow heads, tabular figures via <Td numeric>. */
export function Table({ className, children, ...rest }: HTMLAttributes<HTMLTableElement>) {
  return (
    <table {...rest} className={cx('w-full border-collapse text-sm', className)}>
      {children}
    </table>
  );
}

interface CellProps extends TdHTMLAttributes<HTMLTableCellElement> {
  numeric?: boolean;
}

export function Th({ numeric, className, children, ...rest }: CellProps) {
  return (
    <th
      scope="col"
      {...rest}
      className={cx('eyebrow border-b border-line-strong px-2 py-1.5 first:pl-0 last:pr-0', numeric ? 'text-right' : 'text-left', className)}
    >
      {children}
    </th>
  );
}

export function Td({ numeric, className, children, ...rest }: CellProps) {
  return (
    <td
      {...rest}
      className={cx('border-b border-line px-2 py-1.5 align-baseline first:pl-0 last:pr-0', numeric && 'num text-right', className)}
    >
      {children}
    </td>
  );
}
