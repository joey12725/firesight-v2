// The wrapper every chart on a results view sits in. It gives the chart its words (a
// quiet title, a one-sentence takeaway computed from the data, a caption with the
// definitions) and its exits: the same data as an accessible table, a PNG, a CSV, and
// "Explain this", which hands the chart's key numbers to the assistant.
import { useId, useRef, useState, type ReactNode } from 'react';
import { askAssistant } from '@/assistant';
import { toast } from '@/state/ui-store';
import { IconButton } from '../Button';
import { cx } from '../cx';
import { Table, Td, Th } from '../misc';
import { chartToPng, copyOrDownloadPng, downloadBlob, tableToCsv, type TableData } from './export';

export interface ChartCardProps {
  /** What is plotted, as a short noun phrase: "Net worth among the living". */
  title: string;
  /** The one sentence a reader should leave with, computed from the data. Plain text is also used for the exported image. */
  takeaway?: ReactNode;
  /** Plain-text form of the takeaway for the PNG and for the assistant, when `takeaway` is not a string. */
  takeawayText?: string;
  /** Definitions, sample size, what is not shown. Small print under the chart. */
  caption?: ReactNode;
  captionText?: string;
  /** Controls that change this chart's encoding (linear/log, which series). View-wide filters do not belong here. */
  controls?: ReactNode;
  /** The plotted data as rows. Enables the table view and the CSV download. */
  table?: () => TableData;
  /** File name stem for downloads. */
  filename?: string;
  /** Enables "Explain this": the prompt and the chart's key numbers (a few hundred bytes of JSON). */
  explain?: { prompt?: string; context: () => unknown };
  /** Hold the previous picture at reduced strength while a new result is on its way. */
  dimmed?: boolean;
  /** Smaller type and no toolbar, for a chart that supports another. */
  quiet?: boolean;
  className?: string;
  children: ReactNode;
}

const numberCell = (v: number) => (Number.isInteger(v) ? v.toLocaleString('en-US') : v.toLocaleString('en-US', { maximumFractionDigits: 3 }));

export function ChartCard({ title, takeaway, takeawayText, caption, captionText, controls, table, filename, explain, dimmed, quiet, className, children }: ChartCardProps) {
  const [asTable, setAsTable] = useState(false);
  const body = useRef<HTMLDivElement>(null);
  const id = useId();
  const stem = filename ?? title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
  const takeawayPlain = takeawayText ?? (typeof takeaway === 'string' ? takeaway : undefined);
  const captionPlain = captionText ?? (typeof caption === 'string' ? caption : undefined);

  const copyImage = async () => {
    const root = body.current;
    if (!root) return;
    try {
      const outcome = await copyOrDownloadPng(() => chartToPng(root, { title, takeaway: takeawayPlain, caption: captionPlain }), `${stem}.png`);
      toast({ title: outcome === 'copied' ? 'Chart copied as an image' : 'Chart saved as an image', tone: 'good' });
    } catch (error) {
      toast({ title: 'The chart could not be copied', description: error instanceof Error ? error.message : undefined, tone: 'bad' });
    }
  };

  const data = asTable && table ? table() : null;

  return (
    <section aria-labelledby={`${id}-title`} className={cx('min-w-0', className)} data-chart-card={stem}>
      <header className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <h3 id={`${id}-title`} className={cx('font-medium text-ink-2', quiet ? 'text-xs' : 'text-sm')}>
            {title}
          </h3>
          {takeaway && <p className={cx('mt-1 max-w-[44rem] text-pretty text-ink-1', quiet ? 'text-sm' : 'text-base leading-snug font-medium tracking-tight sm:text-lg sm:leading-snug')}>{takeaway}</p>}
        </div>
        {!quiet && (
          <div className="-mr-1.5 -mt-1 flex shrink-0 items-center">
            {explain && (
              <IconButton
                icon="sparkles"
                size="sm"
                label="Explain this chart"
                onClick={() => askAssistant(explain.prompt ?? `Explain the "${title}" chart: what it shows for my plan and what I should take from it.`, { chart: title, takeaway: takeawayPlain, ...(explain.context() as object) })}
              />
            )}
            {table && <IconButton icon="table" size="sm" label={asTable ? 'Show the chart' : 'View as a table'} pressed={asTable} onClick={() => setAsTable(!asTable)} />}
            <IconButton icon="image" size="sm" label="Copy the chart as an image" disabled={asTable} onClick={() => void copyImage()} />
            {table && (
              <IconButton
                icon="download"
                size="sm"
                label="Download the data (CSV)"
                onClick={() => downloadBlob(new Blob([tableToCsv(table())], { type: 'text/csv;charset=utf-8' }), `${stem}.csv`)}
              />
            )}
          </div>
        )}
      </header>
      {controls && !asTable && <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2">{controls}</div>}
      <div ref={body} className={cx('mt-3 transition-opacity duration-[var(--duration-slow)]', dimmed && 'opacity-60')}>
        {data ? <DataTable data={data} /> : children}
      </div>
      {caption && <p className="mt-2 max-w-[48rem] text-xs leading-relaxed text-pretty text-ink-3">{caption}</p>}
    </section>
  );
}

/** The accessible twin of a chart: every plotted value, in a scrollable table. */
export function DataTable({ data, maxHeight = 360 }: { data: TableData; maxHeight?: number }) {
  return (
    <div className="scroll-thin overflow-auto rounded-md border border-line" style={{ maxHeight }} tabIndex={0} role="region" aria-label={data.caption}>
      <Table className="text-xs">
        <caption className="sr-only">{data.caption}</caption>
        <thead className="sticky top-0 bg-surface-1">
          <tr>
            {data.columns.map((c, i) => (
              <Th key={c.key} numeric={c.align !== 'left' && i > 0} className="whitespace-nowrap !px-3 first:!pl-3 last:!pr-3">
                {c.label}
              </Th>
            ))}
          </tr>
        </thead>
        <tbody>
          {data.rows.map((row, r) => (
            <tr key={r}>
              {data.columns.map((c, i) => {
                const v = row[c.key];
                const text = v === null || v === undefined ? '' : typeof v === 'number' ? (c.format ?? numberCell)(v) : v;
                return (
                  <Td key={c.key} numeric={c.align !== 'left' && i > 0} className="whitespace-nowrap !px-3 first:!pl-3 last:!pr-3">
                    {text}
                  </Td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </Table>
    </div>
  );
}
