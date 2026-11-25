// The lanes beneath a life's timeline, on the same age axis and crosshair: what came in,
// went out and was taxed each year; the accounts by kind; and the market this life lived
// through. Together they show why the line above went the way it did.
import { scaleLog } from 'd3-scale';
import { useMemo, type ReactNode } from 'react';
import { moneyCompact } from '@/ui';
import {
  AgeAxis,
  ChartFrame,
  Cursor,
  EndLabels,
  Legend,
  PlotClip,
  SvgLayer,
  TipNote,
  TipRow,
  TipTitle,
  ValueAxis,
  linearScale,
  niceDomain,
  useChartSync,
  useChartTheme,
  useTimeRange,
  withAlpha,
  type EndLabel,
  type Scale,
  type TableData,
} from '@/ui/charts';
import { StackLegend, StackedAreas, type StackLine, type StackSeries } from '@/ui/charts/ext/cashflow-StackedAreas';
import { ACCOUNT_COLOR } from '../cash-flow/model';
import { yearEnds } from './Timeline';
import { taxYears, type Life } from './life';

interface LaneSeries {
  key: string;
  label: string;
  /** Short name for the label at the line's end. */
  end: string;
  color: string;
  /** One value per point. Null breaks the line. */
  values: (number | null)[];
  strong?: boolean;
  dash?: boolean;
  /** Draw as steps that change halfway between points (a rate that holds for a whole year). */
  step?: boolean;
}

interface LineLaneProps {
  life: Life;
  /** Age of each point. */
  ages: number[];
  /** The point that stands for each year index (the year itself for annual data, its last month for monthly). */
  pointOfYear: number[];
  series: LaneSeries[];
  format: (v: number) => string;
  tick: 'money' | ((v: number) => string);
  title: string;
  log?: boolean;
  /** A shaded span of ages with a label (the worst fall). */
  shade?: { from: number; to: number; label: string } | null;
  selected?: number | null;
  onSelect?: (year: number) => void;
  note?: (year: number) => ReactNode;
  label: string;
  height?: number;
}

function logScale(domain: [number, number], range: [number, number]): Scale {
  const s = scaleLog().domain(domain).range(range);
  const scale = ((v: number) => s(Math.max(v, domain[0] / 10))) as Scale;
  scale.invert = (px) => s.invert(px);
  scale.domain = domain;
  scale.range = range;
  scale.kind = 'symlog';
  scale.ticks = () => {
    const out: number[] = [];
    for (let p = Math.ceil(Math.log2(domain[0])); 2 ** p <= domain[1]; p++) out.push(2 ** p);
    // Too many doublings for the height: keep every other one, anchored on 1.
    return out.length > 6 ? out.filter((v) => Math.round(Math.log2(v)) % 2 === 0) : out;
  };
  return scale;
}

function LineLane({ life, ages, pointOfYear, series, format, tick, title, log, shade, selected, onSelect, note, label, height = 200 }: LineLaneProps) {
  const sync = useChartSync();
  const { time } = life;
  const range = useTimeRange(time, sync);
  const { first, last } = range;
  const index = Array.from({ length: last - first + 1 }, (_, k) => first + k);
  const [lo, hi] = range.domain;
  const points: number[] = [];
  ages.forEach((a, p) => {
    if (a >= lo - 0.1 && a <= hi + 0.1) points.push(p);
  });
  let top = -Infinity;
  let bottom = Infinity;
  for (const s of series)
    for (const p of points) {
      const v = s.values[p];
      if (v === null || v === undefined) continue;
      top = Math.max(top, v);
      bottom = Math.min(bottom, v);
    }
  if (!Number.isFinite(top)) {
    top = 1;
    bottom = 0;
  }

  return (
    <ChartFrame height={height} margin={{ top: 30, right: 76, bottom: life.hasAges ? 36 : 22, left: 52 }} aria-label={label}>
      {({ plot, theme }) => {
        const x = linearScale([lo, hi], [plot.x, plot.right]);
        const y = log ? logScale([Math.min(0.5, bottom * 0.9), Math.max(2, top * 1.1)], [plot.bottom, plot.y]) : linearScale(niceDomain(Math.min(0, bottom), top * 1.04, 3), [plot.bottom, plot.y]);
        const xs = index.map((i) => x(time.ages[i] as number));
        const path = (row: (number | null)[], step = false) => {
          let d = '';
          let pen = false;
          let before = 0;
          for (const p of points) {
            const v = row[p];
            if (v === null || v === undefined) {
              pen = false;
              continue;
            }
            const px = x(ages[p] as number);
            if (!pen) d += `M${px.toFixed(1)},${y(v).toFixed(1)}`;
            else if (step) d += `H${((before + px) / 2).toFixed(1)}V${y(v).toFixed(1)}H${px.toFixed(1)}`;
            else d += `L${px.toFixed(1)},${y(v).toFixed(1)}`;
            before = px;
            pen = true;
          }
          return d;
        };
        const ends: EndLabel[] = series.flatMap((s) => {
          for (let k = points.length - 1; k >= 0; k--) {
            const v = s.values[points[k] as number];
            if (v !== null && v !== undefined) return [{ key: s.key, y: Math.min(plot.bottom, Math.max(plot.y, y(v))), text: s.end, color: s.color, strong: s.strong }];
          }
          return [];
        });
        const selectedX = selected !== null && selected !== undefined && selected >= first && selected <= last ? x(time.ages[selected] as number) : null;
        const shadeX = shade ? [Math.max(plot.x, x(shade.from)), Math.min(plot.right, x(shade.to))] : null;
        return (
          <>
            <SvgLayer>
              {shade && shadeX && (shadeX[1] as number) > (shadeX[0] as number) && (
                <g>
                  <rect x={shadeX[0]} y={plot.y} width={(shadeX[1] as number) - (shadeX[0] as number)} height={plot.height} fill={withAlpha(theme.status.depleted, theme.dark ? 0.18 : 0.1)} />
                  <text x={(shadeX[0] as number) > plot.right - 110 ? (shadeX[1] as number) : (shadeX[0] as number)} y={plot.y - 5} fontSize={10} fill={theme.annotation} textAnchor={(shadeX[0] as number) > plot.right - 110 ? 'end' : 'start'}>
                    {shade.label}
                  </text>
                </g>
              )}
              <ValueAxis y={y} format={tick} title={title} titleY={10} count={3} />
              <AgeAxis x={x} time={time} years={life.hasAges} label={life.hasAges ? 'Age' : 'Year'} />
              <PlotClip bleed={2}>
                {series.map((s) => (
                  <path key={s.key} d={path(s.values, s.step)} fill="none" stroke={s.color} strokeWidth={s.strong ? 2 : 1.5} strokeDasharray={s.dash ? '5 4' : undefined} strokeLinejoin="round" strokeLinecap="round" />
                ))}
              </PlotClip>
              <EndLabels labels={ends} />
              {selectedX !== null && <line x1={selectedX} x2={selectedX} y1={plot.y} y2={plot.bottom} stroke={theme.ink.primary} strokeWidth={1} strokeOpacity={0.55} />}
            </SvgLayer>
            <Cursor
              count={index.length}
              xOf={(k) => xs[k] ?? 0}
              label={label}
              onBrush={(a, b) => range.set([time.ages[index[a] as number] as number, time.ages[index[b] as number] as number])}
              onSelect={onSelect ? (k) => onSelect(index[k] as number) : undefined}
              clickable={onSelect !== undefined}
              describe={(k) => {
                const i = index[k] as number;
                const p = pointOfYear[i] ?? 0;
                return `${life.hasAges ? `Age ${Math.floor(time.ages[i] as number)}, ` : ''}${time.years[i]}: ${series
                  .map((s) => {
                    const v = s.values[p];
                    return v === null || v === undefined ? null : `${s.label} ${format(v)}`;
                  })
                  .filter(Boolean)
                  .join(', ')}.`;
              }}
              marks={(k) => {
                const p = pointOfYear[index[k] as number] ?? 0;
                return (
                  <g>
                    {series.map((s) => {
                      const v = s.values[p];
                      return v === null || v === undefined ? null : <circle key={s.key} cx={xs[k]} cy={y(v)} r={s.strong ? 4 : 3} fill={s.color} stroke={theme.surface} strokeWidth={2} />;
                    })}
                  </g>
                );
              }}
              tooltip={(k) => {
                const i = index[k] as number;
                const p = pointOfYear[i] ?? 0;
                return (
                  <>
                    <TipTitle detail={life.hasAges ? String(time.years[i]) : undefined}>{life.hasAges ? `Age ${Math.floor(time.ages[i] as number)}` : time.years[i]}</TipTitle>
                    {series.map((s) => {
                      const v = s.values[p];
                      return v === null || v === undefined ? null : <TipRow key={s.key} color={s.color} swatch={s.dash ? 'dash' : 'line'} label={s.label} value={format(v)} strong={s.strong} />;
                    })}
                    {note && <TipNote>{note(i)}</TipNote>}
                  </>
                );
              }}
            />
          </>
        );
      }}
    </ChartFrame>
  );
}

// ---- income, spending and tax ---------------------------------------------------------

function useFlowSeries(life: Life): LaneSeries[] {
  const theme = useChartTheme();
  return useMemo(() => {
    // Part-years (the first, and the year of the last death) would read as a collapse: leave them out.
    const full = (key: string) => life.annual(key).map((v, y) => (life.fullYear[y] ? v : null));
    const planned = full('spend_planned');
    const spending = full('spending');
    const differs = planned.some((p, y) => p !== null && spending[y] !== null && Math.abs(p - (spending[y] as number)) > Math.max(500, p * 0.02));
    return [
      { key: 'income', label: 'Income', end: 'Income', color: theme.categorical[0] as string, values: full('gross_income') },
      { key: 'spending', label: 'Spending', end: 'Spending', color: theme.categorical[1] as string, values: spending, strong: true },
      ...(differs ? [{ key: 'planned', label: 'Spending the plan intended', end: 'Plan', color: theme.categorical[1] as string, values: planned, dash: true }] : []),
      { key: 'tax', label: 'Tax', end: 'Tax', color: theme.categorical[4] as string, values: full('total_tax') },
    ];
  }, [life, theme]);
}

export function flowLaneTable(life: Life): TableData {
  const fmt = (v: number) => moneyCompact(v);
  return {
    caption: `Life #${life.trace.path_index}: income, spending, planned spending and tax by year, as recorded (part-years are not annualised). Today's dollars`,
    columns: [
      { key: 'age', label: life.hasAges ? 'Age' : 'Year' },
      { key: 'year', label: 'Year', format: String },
      { key: 'months', label: 'Months' },
      { key: 'income', label: 'Income', format: fmt },
      { key: 'spending', label: 'Spending', format: fmt },
      { key: 'planned', label: 'Spending the plan intended', format: fmt },
      { key: 'tax', label: 'Tax', format: fmt },
    ],
    rows: life.time.years.map((year, y) => ({
      age: Math.floor(life.time.ages[y] ?? 0),
      year,
      months: life.trace.annual.months[y] ?? null,
      income: life.annual('gross_income')[y] ?? null,
      spending: life.annual('spending')[y] ?? null,
      planned: life.annual('spend_planned')[y] ?? null,
      tax: life.annual('total_tax')[y] ?? null,
    })),
  };
}

export function FlowLaneLegend({ life }: { life: Life }) {
  const series = useFlowSeries(life);
  return (
    <div data-chart-legend>
      <Legend className="mb-1" items={series.map((s) => ({ key: s.key, label: s.label, color: s.color, swatch: s.dash ? ('dash' as const) : ('line' as const) }))} />
    </div>
  );
}

export function FlowLane({ life, selected, onSelect }: { life: Life; selected?: number | null; onSelect?: (year: number) => void }) {
  const series = useFlowSeries(life);
  const identity = useMemo(() => life.time.ages.map((_, y) => y), [life]);
  return (
    <LineLane
      life={life}
      ages={life.time.ages}
      pointOfYear={identity}
      series={series}
      format={moneyCompact}
      tick="money"
      title="A year, today's dollars"
      selected={selected}
      onSelect={onSelect}
      label={`Life number ${life.trace.path_index}: income, spending and tax by year`}
    />
  );
}

// ---- the accounts ---------------------------------------------------------------------

function useAccountSeries(life: Life): { series: StackSeries[]; ends: number[] } {
  const theme = useChartTheme();
  return useMemo(() => {
    const ends = yearEnds(life);
    const row = (key: string, label: string, short: string, color: number, side: 'up' | 'down' = 'up'): StackSeries => ({ key, label, short, color: theme.categorical[color] as string, values: life.annual(key), side });
    const all = [
      row('balance_cash', 'Cash', 'Cash', ACCOUNT_COLOR.cash),
      row('balance_taxable', 'Taxable accounts', 'Taxable', ACCOUNT_COLOR.taxable),
      row('balance_traditional', 'Pre-tax accounts', 'Pre-tax', ACCOUNT_COLOR.traditional),
      row('balance_roth', 'Roth accounts', 'Roth', ACCOUNT_COLOR.roth),
      row('balance_hsa', 'HSA', 'HSA', ACCOUNT_COLOR.hsa),
      row('property_value', 'Property', 'Property', ACCOUNT_COLOR.property),
      row('mortgage', 'Mortgages', 'Mortgage', ACCOUNT_COLOR.mortgage, 'down'),
      row('debt', 'Other debt', 'Debt', ACCOUNT_COLOR.debt, 'down'),
    ];
    const peak = Math.max(1, ...life.netWorth);
    return { series: all.filter((s) => Math.max(0, ...s.values.map((v) => v ?? 0)) >= peak * 0.01), ends };
  }, [life, theme]);
}

/**
 * The paper value of vested private-company holdings, year by year, while the life has
 * any: up to the year it first reads zero again (sold, or lost), then nothing. Null when
 * this life never holds any.
 */
export function paperValueLine(life: Life): (number | null)[] | null {
  const values = life.annual('equity_paper_value');
  let last = -1;
  values.forEach((v, y) => {
    if (v > 0) last = y;
  });
  if (last < 0) return null;
  return values.map((v, y) => (y <= last + 1 ? v : null));
}

export const PAPER_VALUE_LABEL = 'Startup equity on paper (not in net worth)';

function useAccountLines(life: Life): StackLine[] {
  const theme = useChartTheme();
  return useMemo(() => {
    const paper = paperValueLine(life);
    return [{ key: 'net_worth', label: 'Net worth', color: theme.ink.primary, values: life.annual('net_worth'), strong: true }, ...(paper ? [{ key: 'equity_paper_value', label: PAPER_VALUE_LABEL, end: 'On paper', color: theme.ink.secondary, values: paper, dash: true }] : [])];
  }, [life, theme]);
}

export function accountLaneTable(life: Life): TableData {
  const paper = paperValueLine(life) !== null;
  const keys = ['balance_cash', 'balance_taxable', 'balance_traditional', 'balance_roth', 'balance_hsa', 'property_value', 'mortgage', 'debt', 'net_worth', ...(paper ? ['equity_paper_value'] : [])];
  const labels = ['Cash', 'Taxable accounts', 'Pre-tax accounts', 'Roth accounts', 'HSA', 'Property', 'Mortgages', 'Other debt', 'Net worth', ...(paper ? [PAPER_VALUE_LABEL] : [])];
  return {
    caption: `Life #${life.trace.path_index}: year-end balances by account kind, today's dollars`,
    columns: [{ key: 'age', label: life.hasAges ? 'Age' : 'Year' }, { key: 'year', label: 'Year', format: String }, ...keys.map((key, i) => ({ key, label: labels[i] as string, format: (v: number) => moneyCompact(v) }))],
    rows: life.time.years.map((year, y) => ({ age: Math.floor(life.time.ages[y] ?? 0), year, ...Object.fromEntries(keys.map((key) => [key, life.annual(key)[y] ?? null])) })),
  };
}

export function AccountLaneLegend({ life }: { life: Life }) {
  const { series } = useAccountSeries(life);
  const lines = useAccountLines(life);
  // The dark line is named in the caption; the dashed one needs its own key.
  return <StackLegend className="mb-1" series={series} lines={lines.filter((l) => l.dash)} />;
}

export function AccountLane({ life, selected, onSelect }: { life: Life; selected?: number | null; onSelect?: (year: number) => void }) {
  const { series } = useAccountSeries(life);
  const lines = useAccountLines(life);
  return (
    <StackedAreas
      time={life.time}
      series={series}
      lines={lines}
      title="Year end, today's dollars"
      totals={{ up: 'Owned', down: 'Owed' }}
      selected={selected}
      onSelect={onSelect}
      height={250}
      resetKey={`${life.trace.path_index}:${life.trace.seed}:${life.time.length}`}
      label={`Life number ${life.trace.path_index}: account balances by kind at each year end`}
    />
  );
}

// ---- the market this life lived through ---------------------------------------------

export function marketLaneTable(life: Life): TableData {
  const ends = yearEnds(life);
  const times = (v: number) => `×${v.toFixed(2)}`;
  return {
    caption: `Life #${life.trace.path_index}: growth of one dollar held in this life's investments after inflation (returns only), and the price level, at each year end`,
    columns: [
      { key: 'age', label: life.hasAges ? 'Age' : 'Year' },
      { key: 'year', label: 'Year', format: String },
      { key: 'growth', label: 'Real growth of $1', format: times },
      { key: 'prices', label: 'Price level', format: times },
    ],
    rows: life.time.years.map((year, y) => ({ age: Math.floor(life.time.ages[y] ?? 0), year, growth: life.growth[ends[y] ?? 0] ?? null, prices: life.prices[ends[y] ?? 0] ?? null })),
  };
}

function useMarketSeries(life: Life): LaneSeries[] {
  const theme = useChartTheme();
  return useMemo(
    () => [
      { key: 'growth', label: 'A dollar invested, after inflation', end: 'Invested', color: theme.ink.primary, values: life.growth, strong: true },
      { key: 'prices', label: 'Prices', end: 'Prices', color: theme.categorical[3] as string, values: life.prices },
    ],
    [life, theme],
  );
}

export function MarketLaneLegend({ life }: { life: Life }) {
  const series = useMarketSeries(life);
  return (
    <div data-chart-legend>
      <Legend className="mb-1" items={series.map((s) => ({ key: s.key, label: s.label, color: s.color, swatch: 'line' as const }))} />
    </div>
  );
}

export function MarketLane({ life, selected, onSelect }: { life: Life; selected?: number | null; onSelect?: (year: number) => void }) {
  const series = useMarketSeries(life);
  const ends = useMemo(() => yearEnds(life), [life]);
  const d = life.drawdown;
  const times = (v: number) => `×${v >= 10 ? v.toFixed(0) : v.toFixed(v >= 2 ? 1 : 2)}`;
  return (
    <LineLane
      life={life}
      ages={life.monthAges}
      pointOfYear={ends}
      series={series}
      format={times}
      tick={(v) => `×${v}`}
      title="Multiple of the starting value (each gridline doubles)"
      log
      shade={d && d.depth >= 0.15 ? { from: life.monthAges[d.peak] ?? 0, to: life.monthAges[d.trough] ?? 0, label: `Worst fall: ${Math.round(d.depth * 100)}%` } : null}
      selected={selected}
      onSelect={onSelect}
      note={(i) => {
        const m = ends[i] ?? 0;
        const before = i > 0 ? (ends[i - 1] ?? 0) : 0;
        const g = life.growth[m] ?? 1;
        const g0 = i > 0 ? (life.growth[before] ?? 1) : 1;
        return `Investments returned ${((g / g0 - 1) * 100).toFixed(1)}% after inflation in ${life.time.years[i]}.`;
      }}
      label={`Life number ${life.trace.path_index}: real growth of its investments and the price level`}
    />
  );
}

// ---- the tax bracket, year by year ----------------------------------------------------

const rate = (v: number) => `${(v * 100).toFixed(v > 0 && v < 0.1 ? 1 : 0)}%`;

export function taxLaneTable(life: Life): TableData {
  const rows = taxYears(life);
  return {
    caption: `Life #${life.trace.path_index}: the federal bracket on the next dollar of ordinary income, all tax owed as a share of adjusted gross income, and taxable income, from each year's return`,
    columns: [
      { key: 'age', label: life.hasAges ? 'Age' : 'Year' },
      { key: 'year', label: 'Year', format: String },
      { key: 'marginal', label: 'Rate on the next dollar', format: rate },
      { key: 'where', label: 'Taxed', align: 'left' as const },
      { key: 'effective', label: 'All tax, share of income', format: (v: number) => `${(v * 100).toFixed(1)}%` },
      { key: 'taxable', label: "Taxable income, today's dollars", format: (v: number) => moneyCompact(v) },
    ],
    rows: life.time.years.map((year, y) => ({ age: Math.floor(life.time.ages[y] ?? 0), year, marginal: rows[y]?.marginal ?? null, where: rows[y]?.marginal === null || rows[y]?.marginal === undefined ? '' : rows[y]?.foreign ? 'Abroad' : 'US', effective: rows[y]?.effective ?? null, taxable: rows[y]?.taxable ?? null })),
  };
}

function useTaxSeries(life: Life): LaneSeries[] {
  const theme = useChartTheme();
  return useMemo(() => {
    const rows = taxYears(life);
    return [
      { key: 'marginal', label: rows.some((r) => r.foreign) ? 'Rate on the next dollar (federal bracket, or the flat rate abroad)' : 'Federal bracket on the next dollar', end: 'Bracket', color: theme.ink.primary, values: rows.map((r) => r.marginal), strong: true, step: true },
      { key: 'effective', label: 'All tax, as a share of income', end: 'All tax', color: theme.categorical[4] as string, values: rows.map((r) => r.effective) },
    ];
  }, [life, theme]);
}

export function TaxLaneLegend({ life }: { life: Life }) {
  const series = useTaxSeries(life);
  return (
    <div data-chart-legend>
      <Legend className="mb-1" items={series.map((s) => ({ key: s.key, label: s.label, color: s.color, swatch: 'line' as const }))} />
    </div>
  );
}

export function TaxLane({ life, selected, onSelect }: { life: Life; selected?: number | null; onSelect?: (year: number) => void }) {
  const series = useTaxSeries(life);
  const rows = useMemo(() => taxYears(life), [life]);
  const identity = useMemo(() => life.time.ages.map((_, y) => y), [life]);
  return (
    <LineLane
      life={life}
      ages={life.time.ages}
      pointOfYear={identity}
      series={series}
      format={rate}
      tick={(v) => `${Math.round(v * 100)}%`}
      title="Rate"
      selected={selected}
      onSelect={onSelect}
      note={(i) => {
        const t = rows[i]?.taxable;
        return t === null || t === undefined ? 'No return was filed for this year.' : `Taxable income ${moneyCompact(t)} in today’s dollars.`;
      }}
      label={`Life number ${life.trace.path_index}: federal tax bracket and overall tax rate by year`}
      height={180}
    />
  );
}
