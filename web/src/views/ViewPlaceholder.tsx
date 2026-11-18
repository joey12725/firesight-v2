import type { ReactNode } from 'react';

export interface PlannedChart {
  title: string;
  /** What the chart will let the reader see. */
  shows: string;
}

interface ViewPlaceholderProps {
  /** The question this view answers, in the user's words. */
  question: string;
  intro: string;
  charts: PlannedChart[];
  children?: ReactNode;
}

/**
 * What a view shows until its charts are built: the question it answers and the
 * charts planned for it. Replace the whole component when implementing the view.
 */
export function ViewPlaceholder({ question, intro, charts, children }: ViewPlaceholderProps) {
  return (
    <div className="mx-auto max-w-3xl py-8">
      <p className="eyebrow">This view will answer</p>
      <h2 className="mt-2 text-2xl font-semibold tracking-tight text-balance text-ink-1">{question}</h2>
      <p className="mt-3 max-w-xl text-base leading-relaxed text-pretty text-ink-2">{intro}</p>
      {children}
      <ol className="mt-8 border-t border-line">
        {charts.map((chart, i) => (
          <li key={chart.title} className="grid grid-cols-[2rem_1fr] gap-x-3 border-b border-line py-4 sm:grid-cols-[2rem_14rem_1fr]">
            <span className="num pt-px text-xs text-ink-3">{String(i + 1).padStart(2, '0')}</span>
            <h3 className="text-sm font-medium text-ink-1">{chart.title}</h3>
            <p className="col-start-2 mt-1 text-sm leading-relaxed text-ink-2 sm:col-start-3 sm:mt-0">{chart.shows}</p>
          </li>
        ))}
      </ol>
    </div>
  );
}
