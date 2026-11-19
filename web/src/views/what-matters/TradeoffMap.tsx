// The trade-off map: one measure over two variables, every cell simulated on the same
// lives. Cells are flat (the map is as fine as it was measured, no smoothing), printed
// with their value, with labelled iso-lines on top and the plan as entered marked.
import { useMemo, type ReactNode } from 'react';
import { CanvasLayer, ChartFrame, SvgLayer, TipNote, TipRow, TipTitle, rampCss, textWidth, useChartTheme, type TableData } from '@/ui/charts';
import { GridCursor, contourLines, drawHeatCells, polylineAt, type HeatCell, type Point } from '@/ui/charts/ext/levers-heatmap';
import { count } from '@/ui';
import { METRICS, contourLevels, formatCell, formatContour, formatLevel, formatRange, gridPosition, type MetricId, type Unit, type VarDef } from './model';
import type { GridRun } from './sweeps';

export interface HeatPlotProps {
  xLabels: string[];
  yLabels: string[];
  xTitle: string;
  yTitle: string;
  /** `values[row][col]`; row 0 is drawn at the bottom. */
  values: (number | null)[][];
  /** Cells whose value is borrowed from a coarser pass. */
  provisional?: boolean[][];
  /** Draw iso-lines through every `stride`-th node only (while the fine pass is incomplete). 0 draws none. */
  contourStride?: number;
  contours: { level: number; label: string }[];
  unit: Unit | MetricId;
  /** Which ramp: 'cool' when more is better, 'warm' when more is worse. */
  tone: 'cool' | 'warm';
  /** Fractional cell position of the plan as entered. */
  marker?: { col: number; row: number; label: string } | null;
  label: string;
  describe: (col: number, row: number) => string;
  tooltip?: (col: number, row: number) => ReactNode;
  onSelect?: (col: number, row: number) => void;
  selected?: { col: number; row: number } | null;
  compact?: boolean;
}

/** Position on the ramp for a value: the lowest cell still reads as a colour, not as the page. */
const rampAt = (v: number, lo: number, hi: number) => (hi > lo ? 0.06 + 0.94 * ((v - lo) / (hi - lo)) : 0.6);

export function HeatPlot({ xLabels, yLabels, xTitle, yTitle, values, provisional, contourStride = 1, contours, unit, tone, marker, label, describe, tooltip, onSelect, selected, compact }: HeatPlotProps) {
  const theme = useChartTheme();
  const cols = xLabels.length;
  const rows = yLabels.length;
  const flat = values.flat().filter((v): v is number => v !== null);
  const lo = flat.length ? Math.min(...flat) : 0;
  const hi = flat.length ? Math.max(...flat) : 1;
  const ramp = tone === 'cool' ? theme.sequentialCool : theme.sequential;
  const yLabelWidth = Math.max(28, ...yLabels.map((l) => textWidth(l, 11))) + 10;

  const lines = useMemo(() => {
    if (!contourStride || rows < 2 || cols < 2) return [];
    const sub = values.filter((_, r) => r % contourStride === 0).map((row) => row.filter((_, c) => c % contourStride === 0));
    return contours.flatMap(({ level, label: text }) => contourLines(sub, level).map((line) => ({ level, text, line: line.map(([c, r]): Point => [c * contourStride, r * contourStride]) })));
  }, [values, contours, contourStride, rows, cols]);

  return (
    <ChartFrame
      height={(w) => (rows === 1 ? 96 : Math.round(Math.min(compact ? 240 : 460, Math.max(compact ? 180 : 250, (w - yLabelWidth) * 0.66)) + 58))}
      margin={{ top: 22, right: 4, bottom: compact ? 34 : 40, left: yLabelWidth }}
      aria-label={label}
    >
      {({ plot }) => {
        const cw = plot.width / cols;
        const ch = plot.height / rows;
        const rect = (col: number, row: number) => ({ x: plot.x + col * cw, y: plot.bottom - (row + 1) * ch, width: cw, height: ch });
        const cells: HeatCell[] = [];
        for (let r = 0; r < rows; r++) {
          for (let c = 0; c < cols; c++) {
            const v = values[r]?.[c] ?? null;
            cells.push({ ...rect(c, r), t: v === null ? null : rampAt(v, lo, hi), label: formatCell(unit, v), provisional: provisional?.[r]?.[c] });
          }
        }
        const px = (p: Point): Point => [plot.x + (p[0] + 0.5) * cw, plot.bottom - (p[1] + 0.5) * ch];
        // One label per level, on its longest line; labels of neighbouring levels sit at different points along their lines
        // (always measured from the line's upper end) so near-parallel lines do not stack their labels.
        const longest = new Map<number, { length: number; pts: Point[]; text: string }>();
        const paths = lines.map(({ level, text, line }) => {
          const pts = line.map(px);
          const { length } = polylineAt(pts);
          const best = longest.get(level);
          if (length >= 36 && (!best || length > best.length)) longest.set(level, { length, pts, text });
          return `M${pts.map((p) => `${p[0].toFixed(1)},${p[1].toFixed(1)}`).join('L')}`;
        });
        // A label sits where its line crosses the seam between two rows of cells, so it never covers a cell's printed value.
        const seams = (pts: Point[]): { at: number; point: Point }[] => {
          const out: { at: number; point: Point }[] = [];
          const total = polylineAt(pts).length;
          let walked = 0;
          for (let i = 1; i < pts.length; i++) {
            const p = pts[i - 1] as Point;
            const q = pts[i] as Point;
            const step = Math.hypot(q[0] - p[0], q[1] - p[1]);
            for (let r = 1; r < rows; r++) {
              const y = plot.bottom - r * ch;
              if (p[1] !== q[1] && y >= Math.min(p[1], q[1]) && y <= Math.max(p[1], q[1])) {
                const f = (y - p[1]) / (q[1] - p[1]);
                out.push({ at: total > 0 ? (walked + f * step) / total : 0, point: [p[0] + (q[0] - p[0]) * f, y] });
              }
            }
            walked += step;
          }
          return out;
        };
        const labels = [...longest.entries()]
          .sort((a, b) => a[0] - b[0])
          .map(([level, l], i, all) => {
            const downward = (l.pts[0] as Point)[1] <= (l.pts[l.pts.length - 1] as Point)[1] ? l.pts : [...l.pts].reverse();
            const want = (i + 1) / (all.length + 1);
            const seam = seams(downward).sort((a, b) => Math.abs(a.at - want) - Math.abs(b.at - want))[0];
            return { level, text: l.text, point: seam ? seam.point : polylineAt(downward, want).point };
          });
        const every = cw < 34 ? 2 : 1;
        const mark = marker ? px([marker.col, marker.row]) : null;
        return (
          <>
            <CanvasLayer deps={[values, provisional, lo, hi, tone]} draw={(ctx) => drawHeatCells(ctx, cells, ramp, { surface: theme.surface, empty: theme.grid, font: theme.fontSans })} />
            <SvgLayer>
              <text x={0} y={11} fontSize={11} fill={theme.label}>
                {textWidth(yTitle, 11) > plot.right ? yTitle.split(/[,(]/)[0] : yTitle}
              </text>
              <g fontSize={11} fill={theme.label} style={{ fontVariantNumeric: 'tabular-nums' }}>
                {yLabels.map((l, r) => (
                  <text key={r} x={plot.x - 8} y={plot.bottom - (r + 0.5) * ch + 3.5} textAnchor="end">
                    {l}
                  </text>
                ))}
                {xLabels.map((l, c) =>
                  (c % every === 0 && every === 1) || (every === 2 && c % 2 === (cols - 1) % 2) ? (
                    <text key={c} x={plot.x + (c + 0.5) * cw} y={plot.bottom + 15} textAnchor="middle">
                      {l}
                    </text>
                  ) : null,
                )}
                <text x={plot.x + plot.width / 2} y={plot.bottom + (compact ? 30 : 33)} textAnchor="middle">
                  {textWidth(xTitle, 11) > plot.width + plot.x ? xTitle.split(/[,(]/)[0] : xTitle}
                </text>
              </g>
              {paths.map((d, i) => (
                <g key={i} fill="none" strokeLinejoin="round" strokeLinecap="round">
                  <path d={d} stroke={theme.surface} strokeWidth={4} strokeOpacity={0.85} />
                  <path d={d} stroke={theme.ink.primary} strokeWidth={1.25} />
                </g>
              ))}
              {labels.map((l) => (
                <text key={l.level} x={Math.min(plot.right - textWidth(l.text, 11) / 2 - 2, Math.max(plot.x + textWidth(l.text, 11) / 2 + 2, l.point[0]))} y={l.point[1] + 3.5} textAnchor="middle" fontSize={11} fontWeight={600} fill={theme.ink.primary} stroke={theme.surface} strokeWidth={3.5} strokeLinejoin="round" paintOrder="stroke">
                  {l.text}
                </text>
              ))}
              {mark && marker && (
                <g>
                  <circle cx={mark[0]} cy={mark[1]} r={6.5} fill={theme.surface} />
                  <circle cx={mark[0]} cy={mark[1]} r={4.5} fill={theme.ink.primary} />
                  <text
                    x={mark[0] + (mark[0] > plot.right - 70 ? -10 : 10)}
                    y={mark[1] + (mark[1] < plot.y + 24 ? 16 : -9)}
                    textAnchor={mark[0] > plot.right - 70 ? 'end' : 'start'}
                    fontSize={11}
                    fontWeight={600}
                    fill={theme.ink.primary}
                    stroke={theme.surface}
                    strokeWidth={4}
                    strokeLinejoin="round"
                    paintOrder="stroke"
                  >
                    {marker.label}
                  </text>
                </g>
              )}
            </SvgLayer>
            <GridCursor cols={cols} rows={rows} rect={rect} label={label} describe={describe} tooltip={compact ? undefined : tooltip} onSelect={onSelect} selected={selected} />
          </>
        );
      }}
    </ChartFrame>
  );
}

/** The key to the cell colours: the ramp with the values at its ends. */
export function HeatScale({ lo, hi, unit, tone, note }: { lo: number; hi: number; unit: Unit | MetricId; tone: 'cool' | 'warm'; note?: string }) {
  const theme = useChartTheme();
  const ramp = tone === 'cool' ? theme.sequentialCool : theme.sequential;
  const stops = Array.from({ length: 9 }, (_, i) => rampCss(ramp, 0.06 + 0.94 * (i / 8)));
  return (
    <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-ink-2">
      <span className="num">{formatLevel(unit, lo)}</span>
      <span aria-hidden className="h-2.5 w-28 rounded-xs" style={{ background: `linear-gradient(90deg, ${stops.join(', ')})` }} />
      <span className="num">{formatLevel(unit, hi)}</span>
      {note && <span className="text-ink-3">{note}</span>}
    </div>
  );
}

// ---- the view's map -------------------------------------------------------------------

export interface MapModel {
  values: (number | null)[][];
  provisional: boolean[][];
  /** True when every valid cell of the fine grid has been measured. */
  complete: boolean;
  /** Lives behind the least-sampled measured cell. */
  paths: number;
  lo: number;
  hi: number;
}

/** A run's cells as the values the map draws for one measure, borrowing from the coarse pass where the fine one has not arrived. */
export function mapModel(run: GridRun, metric: MetricId): MapModel {
  const value = (yi: number, xi: number) => run.cells[yi]?.[xi]?.levels[metric].value ?? null;
  let complete = true;
  let paths = Infinity;
  const provisional = run.ys.map(() => run.xs.map(() => false));
  const values = run.ys.map((_, yi) =>
    run.xs.map((_, xi) => {
      const cell = run.cells[yi]?.[xi];
      if (cell) {
        paths = Math.min(paths, cell.paths);
        return cell.levels[metric].value;
      }
      if (run.invalid[yi]?.[xi]) return null;
      complete = false;
      const near = value(Math.min(run.ys.length - 1, Math.round(yi / 2) * 2), Math.min(run.xs.length - 1, Math.round(xi / 2) * 2));
      if (near !== null) (provisional[yi] as boolean[])[xi] = true;
      return near;
    }),
  );
  const flat = values.flat().filter((v): v is number => v !== null);
  return { values, provisional, complete, paths: Number.isFinite(paths) ? paths : 0, lo: flat.length ? Math.min(...flat) : 0, hi: flat.length ? Math.max(...flat) : 1 };
}

export function mapTable(run: GridRun, x: VarDef, y: VarDef, metric: MetricId): TableData {
  const def = METRICS[metric];
  return {
    caption: `${def.axis} for each combination of ${x.label.toLowerCase()} and ${y.label.toLowerCase()}, simulated on the same lives`,
    columns: [
      { key: 'x', label: x.label, align: 'left' },
      { key: 'y', label: y.label, align: 'left' },
      { key: 'value', label: def.label, format: (v) => formatLevel(metric, v) },
      { key: 'lo', label: '95% low', format: (v) => formatLevel(metric, v) },
      { key: 'hi', label: '95% high', format: (v) => formatLevel(metric, v) },
      { key: 'lives', label: 'Lives' },
    ],
    rows: run.ys.flatMap((yv, yi) =>
      run.xs.flatMap((xv, xi) => {
        const cell = run.cells[yi]?.[xi];
        if (!cell) return [];
        const l = cell.levels[metric];
        return [{ x: x.format(xv), y: y.format(yv), value: l.value, lo: l.lo, hi: l.hi, lives: cell.paths }];
      }),
    ),
  };
}

export interface TradeoffMapProps {
  run: GridRun;
  x: VarDef;
  y: VarDef;
  metric: MetricId;
  selected: { col: number; row: number } | null;
  onSelect: (col: number, row: number) => void;
}

export function TradeoffMap({ run, x, y, metric, selected, onSelect }: TradeoffMapProps) {
  const def = METRICS[metric];
  const model = useMemo(() => mapModel(run, metric), [run, metric]);
  const contours = useMemo(() => contourLevels(metric, model.lo, model.hi).map((level) => ({ level, label: formatContour(metric, level) })), [metric, model.lo, model.hi]);
  const coarseReady = run.stage >= 1;
  const col = gridPosition(run.xs, x.current);
  const row = gridPosition(run.ys, y.current);
  const reading = (c: number, r: number) => {
    const cell = run.cells[r]?.[c];
    const where = `${x.short} ${x.format(run.xs[c] as number)}, ${y.short.toLowerCase()} ${y.format(run.ys[r] as number)}`;
    if (!cell) return `${where}: ${run.invalid[r]?.[c] ? 'not a valid plan' : 'not simulated yet'}.`;
    const l = cell.levels[metric];
    return `${where}: ${formatLevel(metric, l.value)}${l.lo !== null && l.hi !== null ? `, 95% interval ${formatRange(metric, l.lo, l.hi)}` : ''}, ${count(cell.paths)} lives.`;
  };
  return (
    <HeatPlot
      xLabels={run.xs.map(x.format)}
      yLabels={run.ys.map(y.format)}
      xTitle={x.axis}
      yTitle={y.axis}
      values={model.values}
      provisional={model.provisional}
      contourStride={model.complete ? 1 : coarseReady ? 2 : 0}
      contours={contours}
      unit={metric}
      tone={def.better > 0 ? 'cool' : 'warm'}
      marker={col !== null && row !== null ? { col, row, label: 'Plan as entered' } : null}
      label={`${def.axis}, by ${x.label.toLowerCase()} and ${y.label.toLowerCase()}`}
      describe={reading}
      selected={selected}
      onSelect={(c, r) => run.cells[r]?.[c] && onSelect(c, r)}
      tooltip={(c, r) => {
        const cell = run.cells[r]?.[c];
        const l = cell?.levels[metric];
        return (
          <>
            <TipTitle>{cell && l ? (def.unit === 'share' ? `${metric === 'holds' ? 'Holds' : 'Runs short'} in ${formatLevel(metric, l.value)}` : `${def.label}: ${formatLevel(metric, l.value)}`) : run.invalid[r]?.[c] ? 'Not a valid plan' : 'Not simulated yet'}</TipTitle>
            <TipRow label={x.short} value={x.format(run.xs[c] as number)} />
            <TipRow label={y.short} value={y.format(run.ys[r] as number)} />
            {cell && l && l.lo !== null && l.hi !== null && <TipRow label="95% interval" value={formatRange(metric, l.lo, l.hi)} />}
            {cell && <TipNote>{count(cell.paths)} lives, the same ones in every cell. Click to open this combination.</TipNote>}
          </>
        );
      }}
    />
  );
}
