// What could go wrong: among the lives that fall short, the first thing that went wrong
// (one cause per life, by the engine's stated rules), as ranked bars with the rule in
// words and a way into the view that helps.
import type { Summary } from '@/engine/output-types';
import { navigateToLife, navigateToView } from '@/state/route';
import { Icon } from '@/ui';
import { ofHundred, useChartTheme, type TableData } from '@/ui/charts';
import { VIEWS } from '../registry';
import { CAUSES } from './text';

export function failureTable(summary: Summary): TableData {
  return {
    caption: 'First cause of failure among simulated lives that fall short of the plan',
    columns: [
      { key: 'cause', label: 'First cause', align: 'left' },
      { key: 'count', label: 'Lives' },
      { key: 'of_failed', label: 'Share of lives that fall short', format: (v) => `${(v * 100).toFixed(1)}%` },
      { key: 'of_all', label: 'Share of all lives', format: (v) => `${(v * 100).toFixed(2)}%` },
    ],
    rows: [...summary.failure.causes].sort((a, b) => b.count - a.count).map((c) => ({ cause: CAUSES[c.cause].label, count: c.count, of_failed: c.share_of_failed, of_all: c.share_of_all })),
  };
}

export function WhatCouldGoWrong({ summary }: { summary: Summary }) {
  const theme = useChartTheme();
  const f = summary.failure;
  const causes = [...f.causes].filter((c) => c.count > 0).sort((a, b) => b.count - a.count);
  const max = Math.max(0.0001, ...causes.map((c) => c.share_of_failed ?? 0));
  const failing = summary.picks.find((p) => p.label === 'failing');
  const depth = summary.headline.years_depleted;
  const when = summary.headline.depletion_age;

  if (f.failed === 0) {
    return (
      <p className="flex items-start gap-2 text-sm text-ink-2">
        <Icon name="checkCircle" className="mt-0.5 text-good" />
        <span>
          None of the {summary.paths.toLocaleString('en-US')} simulated lives falls short. That is a statement about this model and these assumptions, not a guarantee: try a harsher market model or a
          longer life in Assumptions.
        </span>
      </p>
    );
  }

  return (
    <div>
      <ul className="divide-y divide-line border-y border-line">
        {causes.map((c) => {
          const text = CAUSES[c.cause];
          const share = c.share_of_failed ?? 0;
          const view = VIEWS.find((v) => v.id === text.view);
          return (
            <li key={c.cause} className="grid grid-cols-1 gap-x-6 gap-y-1.5 py-3 @2xl:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
              <div className="min-w-0">
                <p className="text-sm font-medium text-ink-1">{text.label}</p>
                <p className="mt-0.5 text-xs leading-relaxed text-pretty text-ink-2">{text.rule}</p>
              </div>
              <div className="min-w-0">
                <div className="flex items-center gap-3">
                  <div className="h-2.5 min-w-0 flex-1" role="img" aria-label={`${Math.round(share * 100)}% of the lives that fall short`}>
                    <div className="h-full rounded-r-[4px]" style={{ width: `${Math.max(0.5, (share / max) * 100)}%`, background: theme.status.depleted }} />
                  </div>
                  <span className="num w-10 shrink-0 text-right text-sm font-medium text-ink-1">{Math.round(share * 100)}%</span>
                </div>
                <div className="mt-1 flex flex-wrap items-baseline justify-between gap-x-3 text-xs text-ink-3">
                  <span>{ofHundred(c.share_of_all).text} of all lives</span>
                  {view && (
                    <button type="button" className="inline-flex items-center gap-1 rounded-xs font-medium text-accent-ink hover:underline" onClick={() => navigateToView(view.id)}>
                      {text.action}
                      <Icon name="arrowRight" size={12} />
                    </button>
                  )}
                </div>
              </div>
            </li>
          );
        })}
      </ul>
      <div className="mt-3 flex flex-wrap items-baseline gap-x-6 gap-y-1 text-xs text-ink-2">
        {when.p50 !== null && (
          <span>
            Money runs short around age <b className="num font-medium text-ink-1">{Math.round(when.p50)}</b>
            {when.p10 !== null && when.p90 !== null && ` (${Math.round(when.p10)} to ${Math.round(when.p90)} in 8 of 10 such lives)`}
            {depth.p50 !== null && (
              <>
                {' '}
                and the shortfall lasts <b className="num font-medium text-ink-1">{Math.round(depth.p50)}</b> {Math.round(depth.p50) === 1 ? 'year' : 'years'} in the middle case.
              </>
            )}
          </span>
        )}
        {failing && (
          <button type="button" className="inline-flex items-center gap-1 rounded-xs font-medium text-accent-ink hover:underline" onClick={() => navigateToLife(failing.path_index, 'failing')}>
            Read a life that ran short
            <Icon name="arrowRight" size={12} />
          </button>
        )}
      </div>
    </div>
  );
}
