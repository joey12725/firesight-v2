import { useState, type ReactNode } from 'react';
import { remove } from '@/state/patches';
import { useUi } from '@/state/ui-store';
import { Button, Icon, IconButton, IssueList, TextInput, cx, type IconName } from '@/ui';
import { set } from '@/state/patches';
import { useEdit, useIssues, useValue } from './hooks';

interface SectionProps {
  id: string;
  icon: IconName;
  title: string;
  /** One line describing the section's contents, shown while it is collapsed. */
  summary: ReactNode;
  /** Path prefixes whose issues count towards this section's badge. */
  paths: string[];
  children: ReactNode;
}

function IssueBadge({ paths }: { paths: string[] }) {
  // Hooks cannot run in a loop of unknown length; sections own at most three prefixes.
  const a = useIssues(paths[0] ?? '/-', true);
  const b = useIssues(paths[1] ?? '/-', true);
  const c = useIssues(paths[2] ?? '/-', true);
  const all = [...a, ...b, ...c];
  const errors = all.filter((i) => i.severity === 'error').length;
  const warnings = all.length - errors;
  if (all.length === 0) return null;
  return (
    <span
      className={cx('num inline-flex h-4 min-w-4 items-center justify-center rounded-full px-1 text-2xs font-medium', errors ? 'bg-bad-soft text-bad' : 'bg-warn-soft text-warn')}
      aria-label={`${errors ? `${errors} error${errors > 1 ? 's' : ''}` : ''}${errors && warnings ? ', ' : ''}${warnings ? `${warnings} warning${warnings > 1 ? 's' : ''}` : ''}`}
    >
      {all.length}
    </span>
  );
}

/** One collapsible section of the plan editor. */
export function Section({ id, icon, title, summary, paths, children }: SectionProps) {
  const open = useUi((s) => s.openSections.includes(id));
  const toggle = useUi((s) => s.toggleSection);
  return (
    <section data-section={id} className="border-b border-line">
      <h3>
        <button
          type="button"
          aria-expanded={open}
          aria-controls={`section-${id}`}
          onClick={() => toggle(id)}
          className="group flex w-full items-start gap-3 px-4 py-3 text-left hover:bg-surface-3/50 focus-visible:outline-offset-[-2px]"
        >
          <Icon name={icon} className={cx('mt-0.5', open ? 'text-accent' : 'text-ink-3')} />
          <span className="min-w-0 flex-1">
            <span className="flex items-center gap-2">
              <span className="text-sm font-medium text-ink-1">{title}</span>
              <IssueBadge paths={paths} />
            </span>
            {!open && <span className="mt-0.5 block truncate text-xs text-ink-3">{summary}</span>}
          </span>
          <Icon name="chevronDown" size={14} className={cx('mt-1 text-ink-3 transition-transform duration-[var(--duration-fast)]', open && 'rotate-180')} />
        </button>
      </h3>
      {open && (
        <div id={`section-${id}`} className="space-y-5 px-4 pb-5 pt-1">
          {children}
        </div>
      )}
    </section>
  );
}

// Paths of elements added in this session: they start expanded so the user can fill them in.
const fresh = new Set<string>();
export const markFresh = (path: string) => void fresh.add(path);

interface ItemProps {
  /** Patch path of the list element, for example `/incomes/@alex_job`. */
  path: string;
  /** Right-aligned key figure in the collapsed row. */
  figure?: ReactNode;
  /** Secondary text after the name in the collapsed row. */
  detail?: ReactNode;
  /** Field under `path` holding the display name; pass null for elements without one. */
  nameKey?: string | null;
  fallbackName?: string;
  removable?: boolean;
  defaultOpen?: boolean;
  children: ReactNode;
}

/** One element of a list (an income, an account, a budget line): a summary row that expands to its fields. */
export function Item({ path, figure, detail, nameKey = 'name', fallbackName = 'Untitled', removable = true, defaultOpen = false, children }: ItemProps) {
  const [open, setOpen] = useState(() => defaultOpen || fresh.delete(path));
  const name = useValue<string | undefined>(nameKey ? `${path}/${nameKey}` : '/-');
  const issues = useIssues(path, true);
  const own = useIssues(path);
  const edit = useEdit();
  const errors = issues.some((i) => i.severity === 'error');
  const title = nameKey ? name || fallbackName : fallbackName;
  return (
    <li data-path={path} className={cx('rounded-md border', open ? 'border-line-strong bg-surface-2' : 'border-line bg-surface-2/60 hover:border-line-strong')}>
      <div className="flex items-center">
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
          className="flex min-w-0 flex-1 items-center gap-2 rounded-md py-2 pl-2.5 pr-2 text-left focus-visible:outline-offset-[-2px]"
        >
          <Icon name="chevronRight" size={12} className={cx('text-ink-3 transition-transform duration-[var(--duration-fast)]', open && 'rotate-90')} />
          <span className="min-w-0 flex-1 truncate text-sm">
            <span className="font-medium text-ink-1">{title}</span>
            {detail && !open && <span className="ml-2 text-xs text-ink-3">{detail}</span>}
          </span>
          {issues.length > 0 && <Icon name={errors ? 'alertCircle' : 'alertTriangle'} size={13} className={errors ? 'text-bad' : 'text-warn'} aria-label={errors ? 'Has errors' : 'Has warnings'} />}
          {figure !== undefined && <span className="num shrink-0 text-sm text-ink-1">{figure}</span>}
        </button>
      </div>
      {open && (
        <div className="space-y-3 border-t border-line px-3 pb-3 pt-3">
          {nameKey && (
            <div className="flex items-end gap-2">
              <label className="min-w-0 flex-1">
                <span className="mb-1 block text-xs font-medium text-ink-2">Name</span>
                <TextInput value={name ?? ''} onChange={(v) => edit(set(`${path}/${nameKey}`, v), { coalesce: `${path}/${nameKey}` })} />
              </label>
              {removable && <IconButton icon="trash" label={`Remove ${title}`} variant="danger" onClick={() => edit(remove(path), { label: `Remove ${title}` })} />}
            </div>
          )}
          <IssueList issues={own} />
          {children}
          {!nameKey && removable && (
            <div className="flex justify-end">
              <Button size="sm" variant="danger" icon="trash" onClick={() => edit(remove(path), { label: `Remove ${title}` })}>
                Remove
              </Button>
            </div>
          )}
        </div>
      )}
    </li>
  );
}

export function ItemList({ children, empty }: { children: ReactNode[]; empty: string }) {
  if (children.length === 0) return <p className="text-xs text-ink-3">{empty}</p>;
  return <ul className="space-y-1.5">{children}</ul>;
}

/** The "+ Add" control under a list. */
export function AddButton({ label, onAdd }: { label: string; onAdd: () => void }) {
  return (
    <Button size="sm" variant="ghost" icon="plus" onClick={onAdd} className="-ml-2">
      {label}
    </Button>
  );
}
