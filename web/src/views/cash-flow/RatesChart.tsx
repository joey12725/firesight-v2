// The saving rate while working and the withdrawal rate in retirement, on one percent
// axis across the life, with the plan's moments marked. Both are ratios of averages.
import {
  AgeAxis,
  ChartFrame,
  Cursor,
  EndLabels,
  Legend,
  PlotClip,
  RuleAnnotations,
  SvgLayer,
  TipNote,
  TipRow,
  TipTitle,
  ValueAxis,
  linearScale,
  niceDomain,
  rulesHeight,
  useAnimated,
  useChartSync,
  useChartTheme,
  useTimeRange,
  type EndLabel,
  type TableData,
} from '@/ui/charts';
import type { StackMoment } from '@/ui/charts/ext/cashflow-StackedAreas';
import type { CashFlowModel } from './model';

const pct = (v: number) => `${(v * 100).toFixed(1)}%`;

export function ratesTable(model: CashFlowModel): TableData {
  const rows = [];
  for (let i = model.flowStart; i <= model.end; i++) {
    rows.push({ age: Math.floor(model.time.ages[i] as number), year: model.time.years[i] ?? null, saving: model.savingRate[i] ?? null, withdrawal: model.withdrawalRate[i] ?? null, working: model.workingShare[i] ?? null });
  }
  return {
    caption: 'Saving as a share of gross income, net withdrawals as a share of investable assets a year earlier, and the share of living households still working, by age',
    columns: [
      { key: 'age', label: 'Age' },
      { key: 'year', label: 'Year', format: String },
      { key: 'saving', label: 'Saved, share of income', format: pct },
      { key: 'withdrawal', label: 'Drawn, share of savings', format: pct },
      { key: 'working', label: 'Still working', format: (v) => `${Math.round(v * 100)}%` },
    ],
    rows,
  };
}

export function RatesLegend() {
  const theme = useChartTheme();
  return (
    <div data-chart-legend>
      <Legend
        className="mb-2"
        items={[
          { key: 'saving', label: 'Saved, as a share of income', color: theme.categorical[0] as string, swatch: 'line' },
          { key: 'withdrawal', label: 'Drawn, as a share of savings', color: theme.categorical[1] as string, swatch: 'line' },
        ]}
      />
    </div>
  );
}

export function RatesChart({ model, moments = [], initial = null }: { model: CashFlowModel; moments?: StackMoment[]; initial?: { rate: number; age: number } | null }) {
  const sync = useChartSync();
  const range = useTimeRange(model.time, sync);
  const { time } = model;
  const rates = useAnimated({ saving: model.savingRate, withdrawal: model.withdrawalRate }, { resetKey: `${time.years[0]}:${time.length}`, duration: 360 });
  const first = Math.max(range.first, model.flowStart);
  const last = Math.min(range.last, model.end);
  const index = Array.from({ length: Math.max(0, last - first + 1) }, (_, k) => first + k);
  let top = 0.05;
  for (const i of index) top = Math.max(top, model.savingRate[i] ?? 0, model.withdrawalRate[i] ?? 0);
  if (initial) top = Math.max(top, initial.rate);

  return (
    <ChartFrame height={(w) => Math.round(Math.min(340, Math.max(260, w * 0.5)))} margin={{ top: moments.length ? rulesHeight(moments.length) + 14 : 22, right: 64, bottom: 36, left: 52 }} aria-label="Saving rate and withdrawal rate by age">
      {({ plot, theme }) => {
        const x = linearScale(range.domain, [plot.x, plot.right]);
        const y = linearScale(niceDomain(0, top * 1.05, 4), [plot.bottom, plot.y]);
        const xs = index.map((i) => x(time.ages[i] as number));
        const colors = { saving: theme.categorical[0] as string, withdrawal: theme.categorical[1] as string };
        const path = (row: (number | null)[]) => {
          let d = '';
          let pen = false;
          index.forEach((i, k) => {
            const v = row[i];
            if (v === null || v === undefined) {
              pen = false;
              return;
            }
            d += `${pen ? 'L' : 'M'}${(xs[k] as number).toFixed(1)},${y(v).toFixed(1)}`;
            pen = true;
          });
          return d;
        };
        const endOf = (row: (number | null)[]) => {
          for (let k = index.length - 1; k >= 0; k--) {
            const v = row[index[k] as number];
            if (v !== null && v !== undefined) return { k, v };
          }
          return null;
        };
        const s = endOf(model.savingRate);
        const w = endOf(model.withdrawalRate);
        const labels: EndLabel[] = [];
        if (w && w.k === index.length - 1) labels.push({ key: 'withdrawal', y: y(w.v), text: 'Drawn', color: colors.withdrawal, strong: true });
        const four = y(0.04);
        return (
          <>
            <SvgLayer>
              <ValueAxis y={y} format="percent" title="Share a year" titleY={moments.length ? 10 : undefined} />
              <AgeAxis x={x} time={time} />
              {moments.length > 0 && <RuleAnnotations items={moments.map((m) => ({ x: x(m.age), label: m.label, detail: m.detail, tone: m.tone ?? 'reference' }))} />}
              {four > plot.y && (
                <g fontSize={10}>
                  <line x1={plot.x} x2={plot.right} y1={Math.round(four) + 0.5} y2={Math.round(four) + 0.5} stroke={theme.annotation} strokeOpacity={0.7} strokeWidth={1} strokeDasharray="4 3" />
                  <text x={plot.x + 4} y={four - 5} fill={theme.label} stroke={theme.surface} strokeWidth={3} paintOrder="stroke">
                    4% a year
                  </text>
                </g>
              )}
              <PlotClip bleed={2}>
                <path d={path(rates.saving)} fill="none" stroke={colors.saving} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
                <path d={path(rates.withdrawal)} fill="none" stroke={colors.withdrawal} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
              </PlotClip>
              {s && s.k < index.length - 1 && (
                <text x={Math.min(plot.right - 4, (xs[s.k] as number) + 6)} y={y(s.v) - 6} fontSize={11} fontWeight={600} fill={theme.ink.primary} stroke={theme.surface} strokeWidth={3} paintOrder="stroke">
                  Saved
                </text>
              )}
              {initial && initial.age >= range.domain[0] && initial.age <= range.domain[1] && (
                <g fontSize={11}>
                  <circle cx={x(initial.age)} cy={y(initial.rate)} r={4.5} fill={theme.surface} stroke={colors.withdrawal} strokeWidth={2} />
                  <text
                    x={x(initial.age) > plot.right - 150 ? x(initial.age) - 9 : x(initial.age) + 9}
                    y={y(initial.rate) + (y(initial.rate) < plot.y + 26 ? 16 : -8)}
                    textAnchor={x(initial.age) > plot.right - 150 ? 'end' : 'start'}
                    fill={theme.ink.primary}
                    stroke={theme.surface}
                    strokeWidth={3}
                    paintOrder="stroke"
                  >
                    Middle life’s first year: {pct(initial.rate)}
                  </text>
                </g>
              )}
              <EndLabels labels={labels} />
            </SvgLayer>
            <Cursor
              count={index.length}
              xOf={(k) => xs[k] ?? 0}
              label="Saving rate and withdrawal rate by age"
              onBrush={(a, b) => range.set([time.ages[index[a] as number] as number, time.ages[index[b] as number] as number])}
              describe={(k) => {
                const i = index[k] as number;
                const sv = model.savingRate[i];
                const wv = model.withdrawalRate[i];
                return `Age ${Math.floor(time.ages[i] as number)}, ${time.years[i]}. ${sv !== null && sv !== undefined ? `Saved ${pct(sv)} of income. ` : ''}${wv !== null && wv !== undefined ? `Drew ${pct(wv)} of savings. ` : ''}${Math.round((model.workingShare[i] ?? 0) * 100)} of 100 living households still working.`;
              }}
              marks={(k) => {
                const i = index[k] as number;
                return (
                  <g>
                    {(['saving', 'withdrawal'] as const).map((key) => {
                      const v = (key === 'saving' ? model.savingRate : model.withdrawalRate)[i];
                      return v === null || v === undefined ? null : <circle key={key} cx={xs[k]} cy={y(v)} r={4} fill={colors[key]} stroke={theme.surface} strokeWidth={2} />;
                    })}
                  </g>
                );
              }}
              tooltip={(k) => {
                const i = index[k] as number;
                const sv = model.savingRate[i];
                const wv = model.withdrawalRate[i];
                return (
                  <>
                    <TipTitle detail={String(time.years[i])}>Age {Math.floor(time.ages[i] as number)}</TipTitle>
                    {sv !== null && sv !== undefined && <TipRow color={colors.saving} label="Saved, of income" value={pct(sv)} />}
                    {wv !== null && wv !== undefined && <TipRow color={colors.withdrawal} label="Drawn, of savings" value={pct(wv)} />}
                    <TipNote>{Math.round((model.workingShare[i] ?? 0) * 100)} of 100 living households are still working.</TipNote>
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
