// The marks of the Equity view, drawn by hand on the chart kit: two success intervals
// on one axis, the four outcomes as one bar of 100 lives, proceeds as a quantile dot
// plot, when the company is sold or fails, cash in and out by age, and the small
// by-outcome tables. Each has a `...Table` twin for the card's table view and CSV.
import type { ReactNode } from 'react';
import type { EquityOutcome, EquitySummary } from '@/engine/output-types';
import { EQUITY_OUTCOMES } from '@/engine/output-types';
import { Table, Td, Th, count, cx, moneyCompact } from '@/ui';
import { AgeAxis, ChartFrame, Cursor, EndLabels, Legend, SvgLayer, TipRow, TipTitle, ValueAxis, createTimeAxis, inkOn, linearScale, niceDomain, textWidth, useChartTheme, type TableData } from '@/ui/charts';
import { QuantileDots, type DotGroup } from '@/ui/charts/ext/outcomes-QuantileDots';
import { XAxis } from '@/ui/charts/ext/outcomes-XAxis';
import { IntervalMark } from '@/ui/charts/ext/scenarios-marks';
import { countsOf, type CashModel, type DecisionRow, type OutcomeCount, type PaysModel, type WhenModel, type WithWithout } from './model';
import { LEFT_OUT_LABEL, OUTCOME_LABEL, age1, holdsText, points, withWithoutDecimals } from './text';

const per100 = (v: number) => String(Math.round(v * 100));
const signedMoney = (v: number) => (Math.abs(v) < 500 ? 'no change' : `${v < 0 ? '−' : '+'}${moneyCompact(Math.abs(v))}`);
/** Signed, rounded first: a change that rounds to nothing is "0.0", never "−0.0" or "+0.0". */
const signedPoints = (v: number) => points(v, true);

// ---- panel 1: with the equity, and without it -------------------------------------------

export interface WithWithoutSide {
  key: 'modelled' | 'zero';
  label: string;
  /** A short qualifier after the label ("on your odds"). */
  tag?: string;
  value: number;
  lo: number;
  hi: number;
  /** The figure printed beside the mark: a decimal when the two sides would otherwise read the same, or disagree with the measured difference. */
  text: string;
}

export function withWithoutSides(w: WithWithout, ownOdds: boolean): WithWithoutSide[] {
  const d = withWithoutDecimals(w);
  return [
    { key: 'modelled', label: 'As modelled', tag: ownOdds ? 'on your odds' : undefined, ...w.modelled, text: holdsText(w.modelled.value, d) },
    { key: 'zero', label: LEFT_OUT_LABEL, ...w.zero, text: holdsText(w.zero.value, d) },
  ];
}

export function WithWithoutChart({ sides }: { sides: WithWithoutSide[] }) {
  const values = sides.flatMap((s) => [s.lo, s.hi, s.value]);
  return (
    <ChartFrame height={(w) => (w < 460 ? 46 : 34) * sides.length + 46} margin={{ top: 6, right: 44, bottom: 40, left: 8 }} aria-label="Lives in which the plan holds, with the equity as modelled and with it left out">
      {({ plot, theme, width }) => {
        const above = width < 460;
        const side = above ? 0 : 190;
        const row = above ? 46 : 34;
        const lo = Math.min(...values);
        const hi = Math.max(...values);
        const pad = Math.max(0.02, (hi - lo) * 0.3);
        const domain = niceDomain(Math.max(0, lo - pad), Math.min(1, hi + pad), 4);
        const x = linearScale(domain, [plot.x + side, plot.right]);
        return (
          <SvgLayer>
            <XAxis x={x} format={per100} count={4} grid from={plot.x + side} title="Lives in which the plan holds, of 100" />
            {sides.map((s, i) => {
              const cy = plot.y + row * i + (above ? 32 : row / 2);
              const strong = s.key === 'modelled';
              const color = strong ? theme.baseline.line : theme.ink.secondary;
              const text = s.text;
              return (
                <g key={s.key}>
                  <text x={plot.x} y={above ? cy - 15 : cy + 4} fontSize={11.5} fontWeight={strong ? 600 : 400} fill={strong ? theme.ink.primary : theme.ink.secondary}>
                    {s.label}
                    {s.tag ? ` · ${s.tag}` : ''}
                  </text>
                  <IntervalMark x={x(s.value)} lo={x(s.lo)} hi={x(s.hi)} y={cy} color={color} />
                  <text x={Math.min(plot.right + 40 - textWidth(text, 11), x(s.hi) + 9)} y={cy + 4} fontSize={11} fontWeight={strong ? 600 : 400} fill={theme.ink.primary} style={{ fontVariantNumeric: 'tabular-nums' }}>
                    {text}
                  </text>
                </g>
              );
            })}
          </SvgLayer>
        );
      }}
    </ChartFrame>
  );
}

export function withWithoutTable(w: WithWithout, sides: WithWithoutSide[], medians: { fi: [number | null, number | null]; wealth: [number | null, number | null] }): TableData {
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;
  return {
    caption: `The plan with the equity as modelled and with it left out (nothing exercised, received or taxed), paired on ${count(w.paths)} of the same lives`,
    columns: [
      { key: 'plan', label: 'Plan', align: 'left' },
      { key: 'holds', label: 'Plan holds', format: pct },
      { key: 'lo', label: '95% low', format: pct },
      { key: 'hi', label: '95% high', format: pct },
      { key: 'fi', label: 'Median age work optional', format: (v) => age1(v) },
      { key: 'wealth', label: 'Median net worth at the end', format: moneyCompact },
    ],
    rows: [
      ...sides.map((s, i) => ({ plan: s.label, holds: s.value, lo: s.lo, hi: s.hi, fi: medians.fi[i] ?? null, wealth: medians.wealth[i] ?? null })),
      { plan: 'Difference (as modelled minus left out)', holds: w.difference.value, lo: w.difference.lo, hi: w.difference.hi, fi: null, wealth: null },
    ],
  };
}

// ---- panel 2: what happens to the company -----------------------------------------------

/** The outcome colours: the first four categorical colours, assigned in the engine's outcome order and never re-assigned. */
export function useOutcomeColors(): Record<EquityOutcome, string> {
  const theme = useChartTheme();
  return Object.fromEntries(EQUITY_OUTCOMES.map((o, i) => [o, theme.categorical[i] as string])) as Record<EquityOutcome, string>;
}

/** One bar of every life, split by what happens to the company, with the count on each segment. */
export function OutcomeBar({ rows, of }: { rows: OutcomeCount[]; of: number }) {
  const colors = useOutcomeColors();
  const drawn = rows.filter((r) => r.lives > 0);
  return (
    <ChartFrame height={54} margin={{ top: 6, right: 0, bottom: 6, left: 0 }} aria-label={`Of ${of} lives: ${rows.map((r) => `${OUTCOME_LABEL[r.outcome].toLowerCase()} in ${r.lives}`).join(', ')}`}>
      {({ plot, theme }) => {
        const gap = 2;
        const unit = (plot.width - gap * Math.max(0, drawn.length - 1)) / Math.max(1, of);
        let x = plot.x;
        return (
          <SvgLayer>
            {drawn.map((r, i) => {
              const w = Math.max(1, r.lives * unit);
              const x0 = x;
              x += w + gap;
              const fill = colors[r.outcome];
              const first = i === 0;
              const last = i === drawn.length - 1;
              const radius = 4;
              // Rounded only at the two ends of the whole bar.
              const d = `M${x0 + (first ? radius : 0)},${plot.y}H${x0 + w - (last ? radius : 0)}${last ? `Q${x0 + w},${plot.y} ${x0 + w},${plot.y + radius}V${plot.bottom - radius}Q${x0 + w},${plot.bottom} ${x0 + w - radius},${plot.bottom}` : `V${plot.bottom}`}H${x0 + (first ? radius : 0)}${first ? `Q${x0},${plot.bottom} ${x0},${plot.bottom - radius}V${plot.y + radius}Q${x0},${plot.y} ${x0 + radius},${plot.y}` : `V${plot.y}`}Z`;
              return (
                <g key={r.outcome}>
                  <path d={d} fill={fill} />
                  {w >= 22 && (
                    <text x={x0 + w / 2} y={plot.y + plot.height / 2 + 5} textAnchor="middle" fontSize={14} fontWeight={600} fill={inkOn(fill)} style={{ fontVariantNumeric: 'tabular-nums' }}>
                      {r.lives}
                    </text>
                  )}
                  {w < 22 && (
                    <text x={Math.min(plot.right - 4, x0 + w / 2)} y={plot.y - 1} textAnchor="middle" fontSize={10} fill={theme.ink.secondary}>
                      {r.lives}
                    </text>
                  )}
                </g>
              );
            })}
          </SvgLayer>
        );
      }}
    </ChartFrame>
  );
}

export function OutcomeLegend({ rows, className }: { rows: OutcomeCount[]; className?: string }) {
  const colors = useOutcomeColors();
  return (
    <div data-chart-legend className={className}>
      <Legend items={rows.map((r) => ({ key: r.outcome, label: OUTCOME_LABEL[r.outcome], color: colors[r.outcome], swatch: 'box' as const, detail: String(r.lives) }))} />
    </div>
  );
}

export function outcomeTable(equity: EquitySummary, rows: OutcomeCount[], of: number): TableData {
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;
  return {
    caption: `What happens to ${equity.company || 'the company'} in ${count(equity.paths)} simulated lives`,
    columns: [
      { key: 'outcome', label: 'Outcome', align: 'left' },
      { key: 'lives', label: `Lives of ${of}` },
      { key: 'paths', label: 'Simulated lives' },
      { key: 'share', label: 'Share', format: pct },
      { key: 'lo', label: '95% low', format: pct },
      { key: 'hi', label: '95% high', format: pct },
    ],
    rows: rows.map((r) => ({ outcome: OUTCOME_LABEL[r.outcome], lives: r.lives, paths: r.paths, share: r.share, lo: r.lo, hi: r.hi })),
  };
}

// ---- panel 3: what the equity pays ------------------------------------------------------

const moneyRange = (lo: number, hi: number) => `${moneyCompact(lo)} to ${moneyCompact(hi)}`;

export function PaysDots({ model }: { model: PaysModel }) {
  const net = model.view === 'net';
  const groups: DotGroup[] = net
    ? [
        { key: 'loss', label: 'Out of pocket', side: 'left', note: 'More went in, as strike and tax, than came back.' },
        { key: 'nothing', label: 'About even', side: 'left', hollow: true, note: `Within ${moneyCompact(model.band)} of zero either way, after cost and tax.` },
      ]
    : [{ key: 'nothing', label: 'No cash', side: 'left', hollow: true, note: 'No cash was ever received.' }];
  return (
    <QuantileDots
      dots={model.dots}
      scale="log"
      groupOf={(v) => {
        const g = v === null ? 'nothing' : model.groupOf(v);
        return g === 'paid' ? null : g;
      }}
      groups={groups}
      format={moneyCompact}
      formatRange={moneyRange}
      axis={{ kind: 'value', format: 'money', title: net ? 'After cost and tax · log scale' : 'Before cost and tax · log scale' }}
      noun={net ? 'What the equity pays after the cost of exercising and tax' : 'What the equity pays before cost and tax'}
      height={210}
    />
  );
}

export function paysTable(equity: EquitySummary, model: PaysModel): TableData {
  const spread = model.view === 'net' ? equity.net_proceeds : equity.gross_proceeds;
  const levels: [string, number | null][] = [
    ['10th percentile', spread.p10],
    ['25th percentile', spread.p25],
    ['Median', spread.p50],
    ['75th percentile', spread.p75],
    ['90th percentile', spread.p90],
  ];
  const rows: Record<string, string | number | null>[] = levels.map(([measure, value]) => ({ measure, value }));
  if (model.view === 'net') {
    rows.push(
      { measure: 'Mean over all lives', value: equity.net_proceeds_mean.value },
      { measure: 'Standard error of the mean', value: equity.net_proceeds_mean.std_error },
      { measure: 'Fewest lives that carry half of all the gains', value: count(equity.net_proceeds_mean_half_from) },
    );
    rows.push({ measure: 'Median among lives that received something', value: equity.net_proceeds_if_paid.p50 }, { measure: 'Median exercise cost among lives that exercised', value: equity.exercise_cost.p50 });
  }
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;
  rows.push(
    { measure: 'Share of lives that received no cash', value: pct(equity.nothing_rate.value) },
    { measure: 'Share of lives out of pocket (net below zero)', value: pct(equity.loss_rate.value) },
    { measure: 'Share of lives that exercised for cash', value: pct(equity.exercised_rate.value) },
    { measure: 'Share of lives with tender proceeds', value: pct(equity.tender_rate.value) },
  );
  return {
    caption: `${model.view === 'net' ? 'Proceeds after the cost of exercising and tax' : 'Proceeds before cost and tax'}, over ${count(equity.paths)} simulated lives, today's dollars`,
    columns: [
      { key: 'measure', label: 'Measure', align: 'left' },
      { key: 'value', label: 'Value', format: (v) => `${v < 0 ? '−' : ''}${moneyCompact(Math.abs(v))}` },
    ],
    rows,
  };
}

// ---- panel 4: when ----------------------------------------------------------------------

export function WhenChart({ model, ages, years }: { model: WhenModel; ages: number[]; years: number[] }) {
  const theme = useChartTheme();
  const time = createTimeAxis({ ages, years });
  const index = Array.from({ length: model.last + 1 }, (_, i) => i);
  const startAge = Math.min(model.ages[0] ?? 0, Math.floor(ages[0] ?? model.ages[0] ?? 0));
  const endAge = (model.ages[model.last] ?? startAge) + 1;
  const top = Math.max(0.05, ...model.liquidity, ...model.failed);
  const series = [
    { key: 'liquidity', label: 'Sold or listed', values: model.liquidity, color: theme.baseline.line, strong: true },
    { key: 'failed', label: 'Failed', values: model.failed, color: theme.ink.tertiary, strong: false },
  ];
  return (
    <ChartFrame height={(w) => Math.min(300, Math.max(220, w * 0.5))} margin={{ top: 26, right: 96, bottom: 36, left: 44 }} aria-label="Share of lives in which the company has been sold or listed, and in which it has failed, by age">
      {({ plot }) => {
        const x = linearScale([startAge, endAge], [plot.x, plot.right]);
        const y = linearScale(niceDomain(0, Math.min(1, top * 1.08), 4), [plot.bottom, plot.y]);
        // Entry i is the share by the end of age `ages[i]`: flat through the year, stepping at its end.
        const path = (values: number[]) => {
          let d = `M${x(startAge).toFixed(1)},${y(0).toFixed(1)}`;
          for (const i of index) d += `H${x((model.ages[i] as number) + 1).toFixed(1)}V${y(values[i] ?? 0).toFixed(1)}`;
          return d;
        };
        const xs = index.map((i) => x((model.ages[i] as number) + 1));
        return (
          <>
            <SvgLayer>
              <ValueAxis y={y} format="share100" title="Lives, of 100, by this age" count={4} />
              <AgeAxis x={x} time={time} />
              {series.map((s) => (
                <path key={s.key} d={path(s.values)} fill="none" stroke={s.color} strokeWidth={2} strokeLinejoin="round" />
              ))}
              <EndLabels labels={series.map((s) => ({ key: s.key, y: y(s.values[model.last] ?? 0), text: s.label, color: s.color, strong: s.strong }))} />
            </SvgLayer>
            <Cursor
              count={index.length}
              xOf={(k) => xs[k] ?? 0}
              label="The company's fate by age"
              describe={(k) => `By age ${(model.ages[k] as number) + 1}: sold or listed in ${per100(model.liquidity[k] ?? 0)} of 100 lives, failed in ${per100(model.failed[k] ?? 0)}.`}
              marks={(k) => (
                <g>
                  {series.map((s) => (
                    <circle key={s.key} cx={xs[k]} cy={y(s.values[k] ?? 0)} r={4} fill={s.color} stroke={theme.surface} strokeWidth={2} />
                  ))}
                </g>
              )}
              tooltip={(k) => (
                <>
                  <TipTitle detail={String(time.yearAtAge((model.ages[k] as number) + 1))}>By age {(model.ages[k] as number) + 1}</TipTitle>
                  {series.map((s) => (
                    <TipRow key={s.key} color={s.color} swatch="line" label={s.label} value={`${per100(s.values[k] ?? 0)} of 100`} strong={s.strong} />
                  ))}
                </>
              )}
            />
          </>
        );
      }}
    </ChartFrame>
  );
}

export function WhenLegend() {
  const theme = useChartTheme();
  return (
    <div data-chart-legend>
      <Legend
        className="mb-1"
        items={[
          { key: 'liquidity', label: 'Sold or listed by this age', color: theme.baseline.line, swatch: 'line' },
          { key: 'failed', label: 'Failed by this age', color: theme.ink.tertiary, swatch: 'line' },
        ]}
      />
    </div>
  );
}

export function whenTable(model: WhenModel): TableData {
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;
  return {
    caption: 'Share of all simulated lives in which the company has been sold or listed, and in which it has failed, by the end of each age',
    columns: [
      { key: 'age', label: 'By the end of age' },
      { key: 'liquidity', label: 'Sold or listed', format: pct },
      { key: 'failed', label: 'Failed', format: pct },
    ],
    rows: model.ages.slice(0, model.last + 1).map((a, i) => ({ age: a, liquidity: model.liquidity[i] ?? 0, failed: model.failed[i] ?? 0 })),
  };
}

// ---- panel 5: the plan, by what happens to the company ----------------------------------

/** A success rate with its interval, on a scale shared by the rows of one table. */
function RateGlyph({ value, lo, hi, domain, color }: { value: number; lo: number; hi: number; domain: [number, number]; color: string }) {
  const theme = useChartTheme();
  const width = 96;
  const height = 16;
  const span = domain[1] - domain[0] || 1;
  const x = (v: number) => 5 + ((Math.min(domain[1], Math.max(domain[0], v)) - domain[0]) / span) * (width - 10);
  return (
    <svg width={width} height={height} aria-hidden className="block">
      <line x1={x(lo)} x2={x(hi)} y1={height / 2} y2={height / 2} stroke={color} strokeWidth={2} strokeLinecap="round" />
      <line x1={x(lo)} x2={x(lo)} y1={height / 2 - 3} y2={height / 2 + 3} stroke={color} strokeWidth={2} strokeLinecap="round" />
      <line x1={x(hi)} x2={x(hi)} y1={height / 2 - 3} y2={height / 2 + 3} stroke={color} strokeWidth={2} strokeLinecap="round" />
      <circle cx={x(value)} cy={height / 2} r={4} fill={color} stroke={theme.surface} strokeWidth={1.5} />
    </svg>
  );
}

function OutcomeName({ outcome, wrap }: { outcome: EquityOutcome; wrap?: boolean }) {
  const colors = useOutcomeColors();
  return (
    <span className={cx('inline-flex gap-2', wrap ? 'items-baseline' : 'items-center whitespace-nowrap')}>
      <span aria-hidden className="inline-block size-2.5 shrink-0 rounded-[2px]" style={{ background: colors[outcome] }} />
      {OUTCOME_LABEL[outcome]}
    </span>
  );
}

const TOO_FEW = 'too few lives to say';

export function ByOutcomeTable({ equity }: { equity: EquitySummary }) {
  const theme = useChartTheme();
  const rows = equity.by_outcome;
  const shown = rows.filter((r) => r.enough && r.success_rate.lo !== null && r.success_rate.hi !== null);
  const lo = Math.min(1, ...shown.map((r) => r.success_rate.lo as number));
  const hi = Math.max(0, ...shown.map((r) => r.success_rate.hi as number));
  const pad = Math.max(0.01, (hi - lo) * 0.15);
  const domain: [number, number] = [Math.max(0, lo - pad), Math.min(1, hi + pad)];
  const of = equity.paths < 100 ? equity.paths : 100;
  // The same whole counts as the bar above: they add up to the lives counted.
  const lives = countsOf(rows.map((r) => r.paths), of);
  const money = (v: number | null) => (v === null ? 'n/a' : `${v < 0 ? '−' : ''}${moneyCompact(Math.abs(v))}`);
  const fi = (v: number | null) => (v === null ? 'never, in most' : age1(v));
  const usable = (r: (typeof rows)[number]) => r.enough && r.success_rate.value !== null && r.success_rate.lo !== null && r.success_rate.hi !== null;
  return (
    <>
      {/* Narrow: one block per outcome with every figure in view. A six-column table would hide the last three behind a scroll with no cue. */}
      <ul className="divide-y divide-line border-y border-line @xl:hidden" aria-label="The plan's results among the lives with each outcome" data-testid="by-outcome-stacked">
        {rows.map((r, i) => {
          const s = r.success_rate;
          return (
            <li key={r.outcome} className={cx('py-2.5', !usable(r) && 'text-ink-3')}>
              <div className="flex items-baseline justify-between gap-3 text-sm">
                <OutcomeName outcome={r.outcome} wrap />
                <span className="num shrink-0 text-xs text-ink-2">
                  {lives[i] ?? 0} of {of} lives
                </span>
              </div>
              {usable(r) ? (
                <dl className="mt-1.5 grid grid-cols-2 gap-x-4 gap-y-1.5 text-xs">
                  {[
                    ['Plan holds, of 100', `${per100(s.value as number)} (${per100(s.lo as number)} to ${per100(s.hi as number)})`],
                    ['Work optional', fi(r.fi_age.p50)],
                    ['Net worth at the end', money(r.terminal_net_worth.p50)],
                    ['Net proceeds', money(r.net_proceeds.p50)],
                  ].map(([term, value]) => (
                    <div key={term}>
                      <dt className="text-ink-3">{term}</dt>
                      <dd className="num text-sm text-ink-1">{value}</dd>
                    </div>
                  ))}
                </dl>
              ) : (
                <p className="mt-1 text-xs">{r.paths === 0 ? 'no simulated life' : TOO_FEW}</p>
              )}
            </li>
          );
        })}
      </ul>
      <div className="scroll-thin -mx-1 hidden overflow-x-auto px-1 @xl:block" tabIndex={0} role="region" aria-label="The plan's results among the lives with each outcome">
        <Table className="min-w-[36rem]">
          <thead>
            <tr>
              <Th>What happens</Th>
              <Th numeric>Lives of {of}</Th>
              <Th numeric>Plan holds, of 100</Th>
              <Th className="hidden @2xl:table-cell">
                <span className="sr-only">95% interval</span>
              </Th>
              <Th numeric>Work optional</Th>
              <Th numeric>Net worth at the end</Th>
              <Th numeric>Net proceeds</Th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => {
              const s = r.success_rate;
              return (
                <tr key={r.outcome} className={cx(!usable(r) && 'text-ink-3')}>
                  <Td className="py-2">
                    <OutcomeName outcome={r.outcome} />
                  </Td>
                  <Td numeric className="py-2">
                    {lives[i] ?? 0}
                  </Td>
                  {usable(r) ? (
                    <>
                      <Td numeric className="py-2 whitespace-nowrap">
                        <span className="font-medium text-ink-1">{per100(s.value as number)}</span>
                        <span className="block text-xs text-ink-3">
                          {per100(s.lo as number)} to {per100(s.hi as number)}
                        </span>
                      </Td>
                      <Td className="hidden py-2 @2xl:table-cell">
                        <RateGlyph value={s.value as number} lo={s.lo as number} hi={s.hi as number} domain={domain} color={theme.baseline.line} />
                      </Td>
                      <Td numeric className="py-2 whitespace-nowrap">
                        {fi(r.fi_age.p50)}
                      </Td>
                      <Td numeric className="py-2 whitespace-nowrap">
                        {money(r.terminal_net_worth.p50)}
                      </Td>
                      <Td numeric className="py-2 whitespace-nowrap">
                        {money(r.net_proceeds.p50)}
                      </Td>
                    </>
                  ) : (
                    <Td colSpan={5} className="py-2 text-xs">
                      {r.paths === 0 ? 'no simulated life' : TOO_FEW}
                    </Td>
                  )}
                </tr>
              );
            })}
          </tbody>
        </Table>
      </div>
    </>
  );
}

export function byOutcomeTable(equity: EquitySummary): TableData {
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;
  return {
    caption: `The plan's results among the lives with each outcome for ${equity.company || 'the company'}; medians within each row, today's dollars`,
    columns: [
      { key: 'outcome', label: 'What happens', align: 'left' },
      { key: 'paths', label: 'Simulated lives' },
      { key: 'holds', label: 'Plan holds', format: pct },
      { key: 'lo', label: '95% low', format: pct },
      { key: 'hi', label: '95% high', format: pct },
      { key: 'fi', label: 'Median age work optional', format: (v) => age1(v) },
      { key: 'retire', label: 'Median retirement age', format: (v) => age1(v) },
      { key: 'wealth', label: 'Median net worth at the end', format: moneyCompact },
      { key: 'net', label: 'Median net proceeds', format: (v) => `${v < 0 ? '−' : ''}${moneyCompact(Math.abs(v))}` },
      { key: 'note', label: 'Note', align: 'left' },
    ],
    rows: equity.by_outcome.map((r) => ({
      outcome: OUTCOME_LABEL[r.outcome],
      paths: r.paths,
      holds: r.enough ? r.success_rate.value : null,
      lo: r.enough ? r.success_rate.lo : null,
      hi: r.enough ? r.success_rate.hi : null,
      fi: r.fi_age.p50,
      retire: r.retirement_age.p50,
      wealth: r.terminal_net_worth.p50,
      net: r.net_proceeds.p50,
      note: r.enough ? '' : TOO_FEW,
    })),
  };
}

// ---- panel 6: cash in and out -----------------------------------------------------------

export function useCashColors() {
  const theme = useChartTheme();
  // Proceeds wear the colour the Cash flow view gives startup equity.
  return { proceeds: theme.categorical[1] as string, exercise: theme.categorical[3] as string, paper: theme.ink.secondary };
}

export function CashLegend() {
  const c = useCashColors();
  return (
    <div data-chart-legend>
      <Legend
        className="mb-1"
        items={[
          { key: 'proceeds', label: 'Cash received, a year', color: c.proceeds, swatch: 'box' },
          { key: 'exercise', label: 'Paid to exercise, a year', color: c.exercise, swatch: 'box' },
          { key: 'paper', label: 'Vested holdings on paper, year end', color: c.paper, swatch: 'dash' },
        ]}
      />
    </div>
  );
}

/** Bars for the year's cash each way on one scale, and the paper value in its own strip beneath: a stock and a flow do not share an axis. */
export function CashChart({ model }: { model: CashModel }) {
  const c = useCashColors();
  const theme = useChartTheme();
  const time = createTimeAxis({ ages: model.ages, years: model.years });
  const flows = Array.from({ length: Math.max(0, model.last - model.first + 1) }, (_, k) => model.first + k);
  const all = Array.from({ length: model.last + 1 }, (_, i) => i);
  const up = Math.max(1, ...flows.map((i) => model.proceeds[i] ?? 0));
  const down = Math.max(0, ...flows.map((i) => model.exercise[i] ?? 0));
  const paperTop = Math.max(1, ...all.map((i) => model.paper[i] ?? 0));
  return (
    <ChartFrame height={340} margin={{ top: 26, right: 12, bottom: 36, left: 52 }} aria-label="Average cash from the equity and paid to exercise options each year, and the paper value of vested holdings, by age">
      {({ plot }) => {
        const stripHeight = Math.round(plot.height * 0.3);
        const gap = 34;
        const flowBottom = plot.bottom - stripHeight - gap;
        const x = linearScale([(model.ages[0] ?? 0) - 0.6, (model.ages[model.last] ?? 1) + 0.6], [plot.x, plot.right]);
        // The exercise side gets room in proportion, but never less than a readable sliver.
        const lowShare = down > 0 ? Math.min(0.45, Math.max(0.16, down / (up + down))) : 0;
        const zero = plot.y + (flowBottom - plot.y) * (1 - lowShare);
        const yUp = linearScale(niceDomain(0, up * 1.05, 3), [zero, plot.y]);
        const yDown = linearScale([0, Math.max(1, down) * 1.1], [zero, flowBottom]);
        const yPaper = linearScale(niceDomain(0, paperTop * 1.05, 2), [plot.bottom, plot.bottom - stripHeight]);
        const step = Math.abs(x((model.ages[1] ?? 1) + 0) - x(model.ages[0] ?? 0)) || 12;
        const bar = Math.max(2, Math.min(24, step - 2));
        const xs = all.map((i) => x(model.ages[i] as number));
        const paperPath = all.map((i, k) => `${k === 0 ? 'M' : 'L'}${(xs[k] as number).toFixed(1)},${yPaper(model.paper[i] ?? 0).toFixed(1)}`).join('');
        const rounded = (cx0: number, y0: number, y1: number, upward: boolean) => {
          const h = Math.abs(y1 - y0);
          const r = Math.min(4, h, bar / 2);
          const l = cx0 - bar / 2;
          const rgt = cx0 + bar / 2;
          return upward
            ? `M${l},${y0}V${y1 + r}Q${l},${y1} ${l + r},${y1}H${rgt - r}Q${rgt},${y1} ${rgt},${y1 + r}V${y0}Z`
            : `M${l},${y0}V${y1 - r}Q${l},${y1} ${l + r},${y1}H${rgt - r}Q${rgt},${y1} ${rgt},${y1 - r}V${y0}Z`;
        };
        const downTicks = down > 0 ? [Math.max(1, down)] : [];
        return (
          <>
            <SvgLayer>
              <ValueAxis y={yUp} format="money" title="A year, today’s dollars" count={3} />
              {down > 0 && <ValueAxis y={yDown} format={(v) => `−${moneyCompact(v)}`} ticks={downTicks} />}
              <line x1={plot.x} x2={plot.right} y1={Math.round(zero) + 0.5} y2={Math.round(zero) + 0.5} stroke={theme.axis} strokeWidth={1} shapeRendering="crispEdges" />
              {flows.map((i) => {
                const cx0 = x(model.ages[i] as number);
                const p = model.proceeds[i] ?? 0;
                const e = model.exercise[i] ?? 0;
                return (
                  <g key={i}>
                    {p > 0 && <path d={rounded(cx0, zero - 1, Math.min(zero - 1.5, yUp(p)), true)} fill={c.proceeds} />}
                    {e > 0 && <path d={rounded(cx0, zero + 1, Math.max(zero + 1.5, yDown(e)), false)} fill={c.exercise} />}
                  </g>
                );
              })}
              <ValueAxis y={yPaper} format="money" title="On paper at year end, not in net worth" titleY={plot.bottom - stripHeight - 8} count={2} />
              <path d={paperPath} fill="none" stroke={c.paper} strokeWidth={2} strokeDasharray="5 4" strokeLinejoin="round" strokeLinecap="round" />
              <AgeAxis x={x} time={time} />
            </SvgLayer>
            <Cursor
              count={all.length}
              xOf={(k) => xs[k] ?? 0}
              label="Cash from the equity by age"
              describe={(k) => `Age ${Math.floor(model.ages[k] as number)}: ${moneyCompact(model.proceeds[k] ?? 0)} received, ${moneyCompact(model.exercise[k] ?? 0)} paid to exercise, ${moneyCompact(model.paper[k] ?? 0)} on paper.`}
              marks={(k) => <circle cx={xs[k]} cy={yPaper(model.paper[k] ?? 0)} r={3.5} fill={c.paper} stroke={theme.surface} strokeWidth={2} />}
              tooltip={(k) => (
                <>
                  <TipTitle detail={String(model.years[k])}>Age {Math.floor(model.ages[k] as number)}</TipTitle>
                  {k >= model.first ? (
                    <>
                      <TipRow color={c.proceeds} swatch="box" label="Cash received" value={moneyCompact(model.proceeds[k] ?? 0)} strong />
                      <TipRow color={c.exercise} swatch="box" label="Paid to exercise" value={moneyCompact(model.exercise[k] ?? 0)} />
                    </>
                  ) : null}
                  <TipRow color={c.paper} swatch="dash" label="On paper at year end" value={moneyCompact(model.paper[k] ?? 0)} />
                </>
              )}
            />
          </>
        );
      }}
    </ChartFrame>
  );
}

export function cashTable(model: CashModel): TableData {
  return {
    caption: "Mean per living household: cash received from the equity and paid to exercise options each year, and the paper value of vested holdings at year end; today's dollars",
    columns: [
      { key: 'age', label: 'Age' },
      { key: 'year', label: 'Year', format: String },
      { key: 'proceeds', label: 'Cash received', format: moneyCompact },
      { key: 'exercise', label: 'Paid to exercise', format: moneyCompact },
      { key: 'paper', label: 'On paper at year end', format: moneyCompact },
    ],
    rows: model.ages.slice(0, model.last + 1).map((a, i) => ({ age: Math.floor(a), year: model.years[i] ?? null, proceeds: i >= model.first ? (model.proceeds[i] ?? 0) : null, exercise: i >= model.first ? (model.exercise[i] ?? 0) : null, paper: model.paper[i] ?? 0 })),
  };
}

// ---- the by-outcome table of a paired comparison (decisions panel, Scenarios) -----------

export function DecisionTable({ rows, label, children }: { rows: DecisionRow[]; label: string; children?: ReactNode }) {
  const paths = rows.reduce((t, r) => t + r.paths, 0);
  const of = paths < 100 ? paths : 100;
  const lives = countsOf(rows.map((r) => r.paths), of);
  return (
    <div className="scroll-thin -mx-1 overflow-x-auto px-1" tabIndex={0} role="region" aria-label={label}>
      <Table className="min-w-[21rem]">
        <thead>
          <tr>
            <Th>What happens</Th>
            <Th numeric>Lives of {of}</Th>
            <Th numeric>Net worth at the end, middle life</Th>
            <Th numeric>Plan holds, per 100</Th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => {
            const ok = r.enough && r.wealth !== null && r.holds !== null;
            return (
              <tr key={r.outcome} className={cx(!ok && 'text-ink-3')}>
                <Td className="py-2">
                  <OutcomeName outcome={r.outcome} wrap />
                </Td>
                <Td numeric className="py-2">
                  {lives[i] ?? 0}
                </Td>
                {ok && r.holds ? (
                  <>
                    <Td numeric className="py-2 whitespace-nowrap">
                      <span className="font-medium text-ink-1">{signedMoney(r.wealth as number)}</span>
                      {/* A decision that acts in few lives moves the average and not the middle life. */}
                      {r.mean && !r.mean.withinNoise && Math.abs(r.mean.value) >= 500 && <span className="block text-xs text-ink-3">average {signedMoney(r.mean.value)}</span>}
                    </Td>
                    <Td numeric className="py-2 whitespace-nowrap">
                      {r.holds.value === 0 && r.holds.lo === 0 && r.holds.hi === 0 ? (
                        // Not one life changed sides: an exact zero, not a difference lost in noise.
                        <span className="text-ink-2">no change</span>
                      ) : (
                        <>
                          <span className={cx(r.holds.withinNoise ? 'text-ink-2' : 'font-medium text-ink-1')}>{signedPoints(r.holds.value)}</span>
                          <span className="block text-xs text-ink-3">
                            {signedPoints(r.holds.lo)} to {signedPoints(r.holds.hi)}
                            {r.holds.withinNoise && <span className="hidden @2xl:inline"> · within noise</span>}
                          </span>
                          {r.holds.withinNoise && <span className="block text-xs text-ink-3 @2xl:hidden">within noise</span>}
                        </>
                      )}
                    </Td>
                  </>
                ) : (
                  <Td colSpan={2} className="py-2 text-xs">
                    {r.paths === 0 ? 'no simulated life' : TOO_FEW}
                  </Td>
                )}
              </tr>
            );
          })}
        </tbody>
      </Table>
      {children}
    </div>
  );
}

export function decisionTableData(rows: DecisionRow[], name: string): TableData {
  return {
    caption: `“${name}” against the plan among the lives with each outcome: median and mean change in net worth at the end (today's dollars) and change in the share of lives in which the plan holds, with its 95% interval`,
    columns: [
      { key: 'outcome', label: 'What happens', align: 'left' },
      { key: 'paths', label: 'Paired lives' },
      { key: 'wealth', label: 'Median change in net worth at the end', format: (v) => `${v < 0 ? '−' : ''}${moneyCompact(Math.abs(v))}` },
      { key: 'mean', label: 'Mean change in net worth at the end', format: (v) => `${v < 0 ? '−' : ''}${moneyCompact(Math.abs(v))}` },
      { key: 'mean_lo', label: 'Mean, 95% low', format: (v) => `${v < 0 ? '−' : ''}${moneyCompact(Math.abs(v))}` },
      { key: 'mean_hi', label: 'Mean, 95% high', format: (v) => `${v < 0 ? '−' : ''}${moneyCompact(Math.abs(v))}` },
      { key: 'holds', label: 'Change in plan holds, per 100', format: (v) => (v * 100).toFixed(1) },
      { key: 'lo', label: '95% low', format: (v) => (v * 100).toFixed(1) },
      { key: 'hi', label: '95% high', format: (v) => (v * 100).toFixed(1) },
      { key: 'note', label: 'Note', align: 'left' },
    ],
    rows: rows.map((r) => ({ outcome: OUTCOME_LABEL[r.outcome], paths: r.paths, wealth: r.wealth, mean: r.mean?.value ?? null, mean_lo: r.mean?.lo ?? null, mean_hi: r.mean?.hi ?? null, holds: r.holds?.value ?? null, lo: r.holds?.lo ?? null, hi: r.holds?.hi ?? null, note: r.enough ? (r.holds?.withinNoise ? 'within noise' : '') : TOO_FEW })),
  };
}
