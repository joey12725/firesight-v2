import { useRef, type ReactNode } from 'react';
import { cx } from './cx';

export interface TabItem<T extends string> {
  id: T;
  label: ReactNode;
}

interface TabsProps<T extends string> {
  tabs: readonly TabItem<T>[];
  active: T;
  onChange: (id: T) => void;
  'aria-label': string;
  /** Prefix for tab and panel ids: tab is `${idBase}-tab-${id}`, panel `${idBase}-panel-${id}`. */
  idBase: string;
  className?: string;
}

/** An underlined tab list. Pair with <TabPanel>. Arrow keys, Home and End move between tabs. */
export function Tabs<T extends string>({ tabs, active, onChange, idBase, className, ...aria }: TabsProps<T>) {
  const ref = useRef<HTMLDivElement>(null);
  const go = (index: number) => {
    const tab = tabs[(index + tabs.length) % tabs.length];
    if (!tab) return;
    onChange(tab.id);
    requestAnimationFrame(() => ref.current?.querySelector<HTMLElement>(`#${idBase}-tab-${tab.id}`)?.focus());
  };
  const current = tabs.findIndex((t) => t.id === active);
  return (
    <div
      ref={ref}
      role="tablist"
      {...aria}
      className={cx('scroll-thin flex gap-5 overflow-x-auto border-b border-line', className)}
      onKeyDown={(e) => {
        if (e.key === 'ArrowRight') go(current + 1);
        else if (e.key === 'ArrowLeft') go(current - 1);
        else if (e.key === 'Home') go(0);
        else if (e.key === 'End') go(tabs.length - 1);
        else return;
        e.preventDefault();
      }}
    >
      {tabs.map((t) => {
        const selected = t.id === active;
        return (
          <button
            key={t.id}
            id={`${idBase}-tab-${t.id}`}
            type="button"
            role="tab"
            aria-selected={selected}
            aria-controls={`${idBase}-panel-${t.id}`}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(t.id)}
            className={cx(
              '-mb-px h-10 shrink-0 border-b-2 text-sm whitespace-nowrap transition-colors duration-[var(--duration-fast)] focus-visible:outline-offset-[-2px]',
              selected ? 'border-accent font-medium text-ink-1' : 'border-transparent text-ink-2 hover:text-ink-1',
            )}
          >
            {t.label}
          </button>
        );
      })}
    </div>
  );
}

export function TabPanel({ idBase, id, children, className }: { idBase: string; id: string; children: ReactNode; className?: string }) {
  return (
    <div id={`${idBase}-panel-${id}`} role="tabpanel" aria-labelledby={`${idBase}-tab-${id}`} tabIndex={0} className={cx('outline-none', className)}>
      {children}
    </div>
  );
}
