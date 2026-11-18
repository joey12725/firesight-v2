import { useRef, type ReactNode } from 'react';
import { cx } from './cx';

export interface Segment<T extends string> {
  value: T;
  label: ReactNode;
  /** Accessible name when the label is not text. */
  title?: string;
}

interface SegmentedControlProps<T extends string> {
  value: T;
  onChange: (value: T) => void;
  options: readonly Segment<T>[];
  'aria-label'?: string;
  'aria-labelledby'?: string;
  id?: string;
  size?: 'sm' | 'md';
  /** Stretch to the container's width with equal segments. */
  fill?: boolean;
  className?: string;
}

/** A radio group drawn as one control. Arrow keys move the selection. */
export function SegmentedControl<T extends string>({
  value,
  onChange,
  options,
  size = 'md',
  fill,
  className,
  id,
  ...aria
}: SegmentedControlProps<T>) {
  const ref = useRef<HTMLDivElement>(null);
  const move = (delta: number) => {
    const index = options.findIndex((o) => o.value === value);
    const next = options[(index + delta + options.length) % options.length];
    if (!next) return;
    onChange(next.value);
    requestAnimationFrame(() => ref.current?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus());
  };
  return (
    <div
      ref={ref}
      id={id}
      role="radiogroup"
      {...aria}
      className={cx('inline-flex rounded-sm border border-line bg-surface-3 p-px', fill && 'flex w-full', className)}
      onKeyDown={(e) => {
        if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
          e.preventDefault();
          move(1);
        } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
          e.preventDefault();
          move(-1);
        }
      }}
    >
      {options.map((o) => {
        const selected = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={o.title}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(o.value)}
            className={cx(
              'inline-flex items-center justify-center gap-1 rounded-[4px] px-2.5 font-medium whitespace-nowrap transition-colors duration-[var(--duration-fast)]',
              size === 'sm' ? 'h-6 text-xs' : 'h-[28px] text-xs',
              fill && 'min-w-0 flex-1 px-1.5',
              selected ? 'bg-surface-2 text-ink-1 shadow-[0_0_0_1px_var(--line-strong)]' : 'text-ink-2 hover:text-ink-1',
            )}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
