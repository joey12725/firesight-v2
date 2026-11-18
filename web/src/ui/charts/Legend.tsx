import type { ReactNode } from 'react';
import { cx } from '../cx';

export interface LegendItem {
  key: string;
  label: ReactNode;
  color: string;
  /** Mirror the mark: 'box' for areas and bars, 'line' for lines, 'dash' for a dashed reference line, 'dot' for dots, 'ring' for hollow dots. */
  swatch?: 'box' | 'line' | 'dash' | 'dot' | 'ring';
  /** Lighter text after the label (a value or share). */
  detail?: ReactNode;
}

/**
 * A legend row. Always present when a chart has two or more series, even when the
 * marks are also labelled directly, so identity never rests on colour matching alone.
 * Text is in ink; only the swatch carries the series colour.
 */
export function Legend({ items, className }: { items: LegendItem[]; className?: string }) {
  return (
    <ul className={cx('flex flex-wrap gap-x-4 gap-y-1 text-xs text-ink-2', className)}>
      {items.map((item) => (
        <li key={item.key} className="flex items-center gap-1.5 whitespace-nowrap">
          <Swatch color={item.color} kind={item.swatch ?? 'box'} />
          <span>{item.label}</span>
          {item.detail !== undefined && <span className="num text-ink-3">{item.detail}</span>}
        </li>
      ))}
    </ul>
  );
}

export function Swatch({ color, kind }: { color: string; kind: NonNullable<LegendItem['swatch']> }) {
  const style =
    kind === 'box'
      ? { width: 10, height: 10, borderRadius: 2, background: color }
      : kind === 'dot'
        ? { width: 8, height: 8, borderRadius: 4, background: color }
        : kind === 'ring'
          ? { width: 8, height: 8, borderRadius: 4, boxShadow: `inset 0 0 0 1.5px ${color}` }
          : kind === 'dash'
            ? { width: 14, height: 0, borderTop: `2px dashed ${color}` }
            : { width: 14, height: 2, borderRadius: 1, background: color };
  return <span aria-hidden className="inline-block shrink-0" style={style} />;
}
