// The tornado: each lever setting as a bar from the plan's level to the level with the
// change, with the 95% interval of the paired difference as a whisker. Settings whose
// interval contains zero are drawn hollow, so a small bar is never mistaken for a
// small certain effect. Pure SVG; one focusable row per setting.
import { useMemo, useState } from 'react';
import { ChartFrame, Legend, SvgLayer, TipNote, TipRow, TipTitle, linearScale, moneyTicks, niceDomain, textWidth, useAnimated, useChartTheme, type TableData } from '@/ui/charts';
import { GROUP_LABEL, METRICS, formatDelta, formatLevel, rankLevers, shortLabel, type Effect, type EffectState, type LeverDef, type LeverGroup, type MetricDef, type MetricId } from './model';
import { effectStatement } from './text';

export interface TornadoBar {
  key: string;
  label: string;
  state: EffectState;
  delta: number | null;
  lo: number | null;
  hi: number | null;
  /** For the tooltip and the accessible name. */
  statement: string;
  /** The level with the change, when known. */
  value?: number | null;
}

export interface TornadoLever {
  id: string;
  label: string;
  bars: TornadoBar[];
}

export interface TornadoGroup {
  id: string;
  title: string;
  levers: TornadoLever[];
}

/** The view's levers and effects as the groups the plot draws. */
export function tornadoGroups(levers: LeverDef[], effects: ReadonlyMap<string, Effect>, metric: MetricId, skipped: Record<string, string> = {}): TornadoGroup[] {
  return (['control', 'world'] as LeverGroup[])
    .map((group) => ({
      id: group,
      title: GROUP_LABEL[group],
      levers: rankLevers(levers, effects, group).map(({ lever, sides }) => ({
        id: lever.id,
        label: lever.label,
        bars: sides.map(({ side, effect }) => ({
          key: side.key,
          label: side.label,
          state: effect.state,
          delta: effect.delta,
          lo: effect.lo,
          hi: effect.hi,
          value: effect.value,
          statement: skipped[side.key] ? `Not tested: ${skipped[side.key]}` : effectStatement(metric, effect),
        })),
      })),
    }))
    .filter((g) => g.levers.length > 0);
}

export function tornadoTable(groups: TornadoGroup[], metric: Pick<MetricDef, 'axis' | 'unit'>, base: number | null, paths: number): TableData {
  const verdict: Record<EffectState, string> = { effect: 'distinguishable from zero', noise: 'within noise', none: 'no change', unknown: 'not available' };
  return {
    caption: `Paired effect of each lever setting on: ${metric.axis}. ${paths.toLocaleString('en-US')} lives each, the same lives as the plan.`,
    columns: [
      { key: 'group', label: 'Kind', align: 'left' },
      { key: 'lever', label: 'Lever', align: 'left' },
      { key: 'setting', label: 'Setting', align: 'left' },
      { key: 'plan', label: 'Plan as entered', format: (v) => formatLevel(metric.unit, v) },
      { key: 'delta', label: 'Paired change', format: (v) => formatDelta(metric.unit, v) },
      { key: 'lo', label: '95% low', format: (v) => formatDelta(metric.unit, v) },
      { key: 'hi', label: '95% high', format: (v) => formatDelta(metric.unit, v) },
      { key: 'verdict', label: 'Reading', align: 'left' },
    ],
    rows: groups.flatMap((g) => g.levers.flatMap((l) => l.bars.map((b) => ({ group: g.title, lever: l.label, setting: b.label, plan: base, delta: b.delta, lo: b.lo, hi: b.hi, verdict: verdict[b.state] })))),
  };
}

const HEAD = 30;
const BAR = 10;

/** A bar from `x0` to `x1` with only the data end (`x1`) rounded. */
function barPath(x0: number, x1: number, y: number, h: number): string {
  const w = Math.abs(x1 - x0);
  const r = Math.min(3, w, h / 2);
  const d = x1 >= x0 ? 1 : -1;
  return `M${x0},${y}H${x1 - d * r}Q${x1},${y} ${x1},${y + r}V${y + h - r}Q${x1},${y + h} ${x1 - d * r},${y + h}H${x0}Z`;
}

export interface TornadoPlotProps {
  groups: TornadoGroup[];
  metric: Pick<MetricDef, 'axis' | 'unit' | 'better'>;
  /** The plan's level: where every bar starts. */
  base: number;
  selected?: string | null;
  onSelect?: (key: string) => void;
  /** No group headings, no tooltip, tighter rows: for the assistant's conversation. */
  compact?: boolean;
}

export function TornadoPlot({ groups, metric, base, selected, onSelect, compact }: TornadoPlotProps) {
  const theme = useChartTheme();
  const [hover, setHover] = useState<string | null>(null);
  const bars = useMemo(() => groups.flatMap((g) => g.levers.flatMap((l) => l.bars)), [groups]);
  const target = useMemo(() => ({ base, bars: Object.fromEntries(bars.map((b) => [b.key, [b.delta ?? 0, b.lo ?? b.delta ?? 0, b.hi ?? b.delta ?? 0]])) }), [bars, base]);
  const drawn = useAnimated(target, { resetKey: `${metric.axis}|${bars.length}` });

  // Rows, top to bottom.
  const TITLE = compact ? 18 : 21;
  const ROW = compact ? 17 : 20;
  const GAP = compact ? 5 : 9;
  const top = compact ? 36 : 40;
  const showHeads = !compact && groups.length > 1;
  let cursor = top;
  const layout: ({ kind: 'head'; y: number; title: string } | { kind: 'title'; y: number; label: string } | { kind: 'bar'; y: number; bar: TornadoBar; lever: TornadoLever })[] = [];
  groups.forEach((g, gi) => {
    if (showHeads) {
      cursor += gi > 0 ? 12 : 0;
      layout.push({ kind: 'head', y: cursor, title: g.title });
      cursor += HEAD;
    }
    for (const lever of g.levers) {
      layout.push({ kind: 'title', y: cursor, label: lever.label });
      cursor += TITLE;
      for (const bar of lever.bars) {
        layout.push({ kind: 'bar', y: cursor, bar, lever });
        cursor += ROW;
      }
      cursor += GAP;
    }
  });
  const height = cursor + 22;

  const domainValues = [base];
  for (const b of bars) {
    if (b.delta === null) continue;
    domainValues.push(base + b.delta, base + (b.lo ?? b.delta), base + (b.hi ?? b.delta));
  }
  let [d0, d1] = niceDomain(Math.min(...domainValues), Math.max(...domainValues), 4);
  if (metric.unit === 'share') {
    if (d1 - d0 < 0.04) [d0, d1] = [base - 0.02, base + 0.02];
    d0 = Math.max(0, d0);
    d1 = Math.min(1, d1);
  }

  const good = theme.status.retired_on_plan;
  const bad = theme.status.depleted;

  return (
    <ChartFrame height={height} margin={{ top: 0, right: 0, bottom: 0, left: 0 }} aria-label={`Effect of each lever on: ${metric.axis}`}>
      {(frame) => {
        const narrow = frame.width < 460;
        const labelW = narrow ? Math.min(150, frame.width * 0.42) : Math.min(210, frame.width * 0.34);
        const pad = narrow ? 36 : 54;
        const x = linearScale([d0, d1], [labelW + pad, frame.width - pad]);
        const ticks = x.ticks(narrow ? 3 : 5);
        const tickFormat = metric.unit === 'dollars' ? moneyTicks(ticks) : (v: number) => (metric.unit === 'share' ? String(Math.round(v * 100)) : metric.unit === 'ratio' ? `${Math.round(v * 100)}%` : String(Math.round(v * 10) / 10));
        const xBase = x(drawn.base);
        const fit = (text: string, size: number, room: number) => {
          if (textWidth(text, size) <= room) return text;
          let t = shortLabel(text);
          if (textWidth(t, size) <= room) return t;
          while (t.length > 4 && textWidth(`${t}…`, size) > room) t = t.slice(0, -1);
          return `${t.trimEnd()}…`;
        };
        const active = hover ?? null;
        const tipRow = layout.find((r) => r.kind === 'bar' && r.bar.key === active);
        return (
          <>
            <SvgLayer>
              <text x={0} y={11} fontSize={11} fill={theme.label}>
                {metric.axis}
              </text>
              <g fontSize={11} style={{ fontVariantNumeric: 'tabular-nums' }}>
                {ticks.map((t) => (
                  <g key={t}>
                    <line x1={Math.round(x(t)) + 0.5} x2={Math.round(x(t)) + 0.5} y1={top - 6} y2={height - 22} stroke={theme.grid} strokeWidth={1} shapeRendering="crispEdges" />
                    <text x={x(t)} y={top - 11} textAnchor="middle" fill={theme.label}>
                      {tickFormat(t)}
                    </text>
                  </g>
                ))}
              </g>
              {layout.map((row) => {
                if (row.kind === 'head') {
                  return (
                    <g key={`h-${row.title}`}>
                      <line x1={0} x2={frame.width} y1={row.y + 0.5} y2={row.y + 0.5} stroke={theme.axis} strokeWidth={1} shapeRendering="crispEdges" />
                      <rect x={0} y={row.y + 1} width={textWidth(row.title.toUpperCase(), 10.5) + 22} height={HEAD - 8} fill={theme.surface} />
                      <text x={0} y={row.y + 17} fontSize={10.5} fontWeight={600} letterSpacing="0.06em" fill={theme.ink.secondary}>
                        {row.title.toUpperCase()}
                      </text>
                    </g>
                  );
                }
                if (row.kind === 'title') {
                  return (
                    <text key={`t-${row.label}`} x={0} y={row.y + 14} fontSize={12.5} fontWeight={500} fill={theme.ink.primary}>
                      {fit(row.label, 12.5, frame.width - 8)}
                    </text>
                  );
                }
                const { bar } = row;
                const [delta, lo, hi] = drawn.bars[bar.key] ?? [0, 0, 0];
                const mid = row.y + ROW / 2;
                const y = mid - BAR / 2;
                const xEnd = x(Math.min(d1, Math.max(d0, drawn.base + (delta ?? 0))));
                const xLo = x(Math.min(d1, Math.max(d0, drawn.base + (lo ?? 0))));
                const xHi = x(Math.min(d1, Math.max(d0, drawn.base + (hi ?? 0))));
                const helps = (bar.delta ?? 0) * metric.better > 0;
                const right = (bar.delta ?? 0) >= 0;
                const lit = active === bar.key || selected === bar.key;
                const labelX = bar.state === 'none' || bar.state === 'unknown' ? xBase + 8 : right ? Math.max(xHi, xEnd) + 6 : Math.min(xLo, xEnd) - 6;
                const anchor = bar.state === 'none' || bar.state === 'unknown' || right ? 'start' : 'end';
                const text = bar.state === 'effect' ? formatDelta(metric.unit, bar.delta) : bar.state === 'noise' ? (narrow ? 'noise' : 'within noise') : bar.state === 'none' ? 'no change' : 'not tested';
                return (
                  <g key={bar.key}>
                    {lit && <rect x={-6} y={row.y} width={frame.width + 12} height={ROW} rx={3} fill={theme.ink.primary} fillOpacity={selected === bar.key ? 0.07 : 0.045} />}
                    <text x={0} y={mid + 4} fontSize={11.5} fill={theme.ink.secondary}>
                      {fit(bar.label, 11.5, labelW - 6)}
                    </text>
                    {bar.state === 'effect' && <path d={barPath(xBase, xEnd, y, BAR)} fill={helps ? good : bad} />}
                    {bar.state === 'noise' && <path d={barPath(xBase, xEnd, y + 0.5, BAR - 1)} fill={theme.surface} stroke={theme.ink.tertiary} strokeWidth={1} />}
                    {(bar.state === 'none' || bar.state === 'unknown') && <circle cx={xBase} cy={mid} r={3} fill={theme.surface} stroke={theme.ink.tertiary} strokeWidth={1.5} />}
                    {(bar.state === 'effect' || bar.state === 'noise') && bar.lo !== null && bar.hi !== null && (
                      <g>
                        <line x1={xLo} x2={xHi} y1={mid} y2={mid} stroke={theme.surface} strokeWidth={4} />
                        <path d={`M${xLo},${mid - 3.5}V${mid + 3.5}M${xLo},${mid}H${xHi}M${xHi},${mid - 3.5}V${mid + 3.5}`} stroke={theme.ink.primary} strokeWidth={1.25} fill="none" />
                      </g>
                    )}
                    <text x={labelX} y={mid + 4} fontSize={11} textAnchor={anchor} fontWeight={bar.state === 'effect' ? 600 : 400} fill={bar.state === 'effect' ? theme.ink.primary : theme.ink.tertiary} style={{ fontVariantNumeric: 'tabular-nums' }}>
                      {text}
                    </text>
                  </g>
                );
              })}
              <line x1={Math.round(xBase) + 0.5} x2={Math.round(xBase) + 0.5} y1={top - 6} y2={height - 18} stroke={theme.ink.secondary} strokeWidth={1} shapeRendering="crispEdges" />
              <text x={Math.min(frame.width - 4, Math.max(4, xBase))} y={height - 4} fontSize={11} textAnchor={xBase > frame.width - 90 ? 'end' : xBase < 90 ? 'start' : 'middle'} fill={theme.ink.secondary}>
                Plan as entered: {formatLevel(metric.unit, base)}
              </text>
            </SvgLayer>
            {layout.map((row) =>
              row.kind === 'bar' ? (
                <button
                  key={row.bar.key}
                  type="button"
                  aria-label={`${row.lever.label}, ${row.bar.label}. ${row.bar.statement}`}
                  aria-pressed={onSelect ? selected === row.bar.key : undefined}
                  disabled={!onSelect}
                  data-lever-bar={row.bar.key}
                  className="absolute rounded-xs bg-transparent outline-offset-0 enabled:cursor-pointer"
                  style={{ left: -6, top: row.y, width: frame.width + 12, height: ROW }}
                  onPointerEnter={() => setHover(row.bar.key)}
                  onPointerLeave={() => setHover((h) => (h === row.bar.key ? null : h))}
                  onFocus={() => setHover(row.bar.key)}
                  onBlur={() => setHover((h) => (h === row.bar.key ? null : h))}
                  onClick={() => onSelect?.(row.bar.key)}
                />
              ) : null,
            )}
            {!compact && tipRow && tipRow.kind === 'bar' && (
              <div role="presentation" className="pointer-events-none absolute z-10 rounded-md border border-line bg-surface-2 px-2.5 py-2 text-xs shadow-overlay" style={{ left: Math.min(labelW, Math.max(0, frame.width - 280)), top: tipRow.y + ROW + 4, width: Math.min(280, frame.width) }}>
                <TipTitle detail={shortLabel(tipRow.bar.label)}>{tipRow.lever.label}</TipTitle>
                <TipRow label="Plan as entered" value={formatLevel(metric.unit, base)} />
                {tipRow.bar.value !== undefined && tipRow.bar.value !== null && <TipRow label="With this change" value={formatLevel(metric.unit, tipRow.bar.value)} />}
                {tipRow.bar.delta !== null && (
                  <TipRow strong label="Paired change" value={`${formatDelta(metric.unit, tipRow.bar.delta)}${tipRow.bar.lo !== null && tipRow.bar.hi !== null ? ` (${formatDelta(metric.unit, tipRow.bar.lo)} to ${formatDelta(metric.unit, tipRow.bar.hi)})` : ''}`} />
                )}
                <TipNote>
                  {tipRow.bar.statement}
                  {onSelect ? ' Click for the detail.' : ''}
                </TipNote>
              </div>
            )}
          </>
        );
      }}
    </ChartFrame>
  );
}

export function TornadoLegend({ className }: { className?: string }) {
  const theme = useChartTheme();
  return (
    <div data-chart-legend className={className}>
      <Legend
        items={[
          { key: 'good', label: 'Better on this measure', color: theme.status.retired_on_plan },
          { key: 'bad', label: 'Worse', color: theme.status.depleted },
          { key: 'noise', label: 'Cannot be told from no effect', color: theme.ink.tertiary, swatch: 'ring' },
          { key: 'ci', label: '95% interval of the paired difference', color: theme.ink.primary, swatch: 'line' },
        ]}
      />
    </div>
  );
}

/** Convenience for the view: the plot for one of this view's measures. */
export function Tornado({ groups, metric, base, selected, onSelect }: { groups: TornadoGroup[]; metric: MetricId; base: number; selected: string | null; onSelect: (key: string) => void }) {
  return <TornadoPlot groups={groups} metric={METRICS[metric]} base={base} selected={selected} onSelect={onSelect} />;
}
