// The search behind "find the number": every value the bisection tested against the
// measure it produced, with each run's 95% interval, the goal as a dashed line and the
// bracket the answer was narrowed to. The reader sees the search, not just its result.
import { ChartFrame, Cursor, PlotClip, SvgLayer, TipNote, TipRow, TipTitle, ValueAxis, linearScale, niceDomain, textWidth, type TableData } from '@/ui/charts';
import { count } from '@/ui';
import { METRICS, formatLevel, formatRange, type Goal, type Probe, type SolveOutcome, type VarDef } from './model';
import { SOLVER_PATHS } from './sweeps';

export function probeTable(probes: Probe[], goal: Goal, variable: VarDef): TableData {
  const def = METRICS[goal.metric];
  return {
    caption: `Values of ${variable.label.toLowerCase()} tested by the search, in order, with the measure each produced on ${count(SOLVER_PATHS)} lives`,
    columns: [
      { key: 'order', label: 'Test' },
      { key: 'x', label: variable.label, format: (v) => variable.format(v) },
      { key: 'value', label: def.label, format: (v) => formatLevel(goal.metric, v) },
      { key: 'lo', label: '95% low', format: (v) => formatLevel(goal.metric, v) },
      { key: 'hi', label: '95% high', format: (v) => formatLevel(goal.metric, v) },
      { key: 'met', label: 'Meets the goal', align: 'left' },
    ],
    rows: probes.map((p) => ({ order: p.order, x: p.x, value: p.level.value, lo: p.level.lo, hi: p.level.hi, met: p.met ? 'yes' : 'no' })),
  };
}

export function ProbeChart({ probes, goal, variable, outcome }: { probes: Probe[]; goal: Goal; variable: VarDef; outcome: SolveOutcome | null }) {
  const def = METRICS[goal.metric];
  const bounds = variable.bounds ?? [0, 1];
  const sorted = [...probes].sort((a, b) => a.x - b.x);
  const ys = [goal.target, ...probes.flatMap((p) => [p.level.value, p.level.lo, p.level.hi]).filter((v): v is number => v !== null)];
  let [y0, y1] = niceDomain(Math.min(...ys), Math.max(...ys), 4);
  if (def.unit === 'share') [y0, y1] = [Math.max(0, y0), Math.min(1, y1)];
  if (y1 - y0 < 1e-9) [y0, y1] = [y0 - 0.01, y1 + 0.01];
  const goalText = `Goal: ${goal.id === 'holds' ? 'at least ' : 'at most '}${formatLevel(goal.metric, goal.target)}`;

  return (
    <ChartFrame height={(w) => Math.min(300, Math.max(220, w * 0.42))} margin={{ top: 30, right: 16, bottom: 40, left: 52 }} aria-label={`Values of ${variable.label.toLowerCase()} tested, against ${def.axis.toLowerCase()}`}>
      {({ plot, theme }) => {
        const x = linearScale(bounds, [plot.x + 10, plot.right - 10]);
        const y = linearScale([y0, y1], [plot.bottom, plot.y]);
        const xs = sorted.map((p) => x(p.x));
        const ticks = x.ticks(plot.width < 380 ? 4 : 6).filter((t) => t >= bounds[0] && t <= bounds[1]);
        const line = sorted.filter((p) => p.level.value !== null).map((p) => `${x(p.x).toFixed(1)},${y(p.level.value as number).toFixed(1)}`);
        const yGoal = Math.round(y(goal.target)) + 0.5;
        const converged = outcome?.status === 'converged' ? outcome : null;
        const current = variable.current !== null && variable.current >= bounds[0] && variable.current <= bounds[1] ? x(variable.current) : null;
        // Order numbers sit above each point; where points crowd (the end of a bisection) only the outer ones are numbered.
        let lastLabel = -Infinity;
        return (
          <>
            <SvgLayer>
              <ValueAxis y={y} format={def.unit === 'share' ? 'share100' : def.unit === 'age' ? 'age' : def.unit === 'dollars' ? 'money' : 'percent'} title={def.axis} titleY={11} count={4} />
              <line x1={plot.x} x2={plot.right} y1={plot.bottom + 0.5} y2={plot.bottom + 0.5} stroke={theme.axis} strokeWidth={1} shapeRendering="crispEdges" />
              <g fontSize={11} fill={theme.label} style={{ fontVariantNumeric: 'tabular-nums' }}>
                {ticks.map((t) => (
                  <g key={t}>
                    <line x1={x(t)} x2={x(t)} y1={plot.bottom} y2={plot.bottom + 4} stroke={theme.axis} strokeWidth={1} />
                    <text x={x(t)} y={plot.bottom + 16} textAnchor="middle" fill={theme.ink.secondary}>
                      {variable.format(t)}
                    </text>
                  </g>
                ))}
                <text x={plot.x + plot.width / 2} y={plot.bottom + 34} textAnchor="middle">
                  {textWidth(variable.axis, 11) > plot.width + 40 ? variable.label : variable.axis}
                </text>
              </g>
              {converged?.band && (
                <rect x={x(Math.max(bounds[0], converged.band[0]))} y={plot.y} width={Math.max(1, x(Math.min(bounds[1], converged.band[1])) - x(Math.max(bounds[0], converged.band[0])))} height={plot.height} fill={theme.ink.primary} fillOpacity={0.06} />
              )}
              {current !== null && (
                <g>
                  <line x1={current} x2={current} y1={plot.y} y2={plot.bottom} stroke={theme.ink.tertiary} strokeWidth={1} />
                  <text x={current + (current > plot.right - 90 ? -5 : 5)} y={plot.bottom - 6} textAnchor={current > plot.right - 90 ? 'end' : 'start'} fontSize={11} fill={theme.ink.secondary} stroke={theme.surface} strokeWidth={3} paintOrder="stroke">
                    Plan as entered
                  </text>
                </g>
              )}
              <line x1={plot.x} x2={plot.right} y1={yGoal} y2={yGoal} stroke={theme.annotation} strokeWidth={1.5} strokeDasharray="5 4" />
              <text x={plot.right} y={yGoal + (yGoal - plot.y < 18 ? 14 : -6)} textAnchor="end" fontSize={11} fontWeight={600} fill={theme.ink.primary} stroke={theme.surface} strokeWidth={4} strokeLinejoin="round" paintOrder="stroke">
                {goalText}
              </text>
              <PlotClip bleed={8}>
                {line.length > 1 && <path d={`M${line.join('L')}`} fill="none" stroke={theme.baseline.line} strokeWidth={2} strokeLinejoin="round" />}
                {sorted.map((p) => {
                  if (p.level.value === null) return null;
                  const px = x(p.x);
                  const py = y(p.level.value);
                  const labelled = px - lastLabel >= 14;
                  if (labelled) lastLabel = px;
                  return (
                    <g key={p.order}>
                      {p.level.lo !== null && p.level.hi !== null && <path d={`M${px - 3},${y(p.level.hi)}H${px + 3}M${px},${y(p.level.hi)}V${y(p.level.lo)}M${px - 3},${y(p.level.lo)}H${px + 3}`} stroke={theme.ink.primary} strokeWidth={1.25} fill="none" />}
                      <circle cx={px} cy={py} r={4.5} fill={p.met ? theme.baseline.line : theme.surface} stroke={p.met ? theme.surface : theme.baseline.line} strokeWidth={2} />
                      {labelled && (
                        <text x={px} y={Math.min(py, p.level.hi !== null ? y(p.level.hi) : py) - 7} textAnchor="middle" fontSize={10} fill={theme.ink.tertiary} style={{ fontVariantNumeric: 'tabular-nums' }}>
                          {p.order}
                        </text>
                      )}
                    </g>
                  );
                })}
              </PlotClip>
              {converged && (
                <g>
                  <line x1={x(converged.safe.x)} x2={x(converged.safe.x)} y1={plot.y - 4} y2={plot.bottom} stroke={theme.ink.primary} strokeWidth={1.5} />
                  <text x={Math.min(plot.right, Math.max(plot.x, x(converged.safe.x)))} y={plot.y - 8} textAnchor={x(converged.safe.x) > plot.right - 60 ? 'end' : x(converged.safe.x) < plot.x + 60 ? 'start' : 'middle'} fontSize={11} fontWeight={600} fill={theme.ink.primary}>
                    {variable.format(converged.safe.x)}
                  </text>
                </g>
              )}
            </SvgLayer>
            <Cursor
              count={sorted.length}
              xOf={(i) => xs[i] ?? 0}
              label={`Search over ${variable.label.toLowerCase()}`}
              describe={(i) => {
                const p = sorted[i] as Probe;
                return `Test ${p.order}: ${variable.format(p.x)} gives ${formatLevel(goal.metric, p.level.value)}, ${p.met ? 'meets' : 'misses'} the goal.`;
              }}
              tooltip={(i) => {
                const p = sorted[i] as Probe;
                return (
                  <>
                    <TipTitle detail={`test ${p.order} of ${probes.length}`}>{variable.format(p.x)}</TipTitle>
                    <TipRow color={theme.baseline.line} label={def.label} value={formatLevel(goal.metric, p.level.value)} strong />
                    <TipRow label="95% interval" value={formatRange(goal.metric, p.level.lo, p.level.hi)} />
                    <TipNote>{p.met ? 'Meets the goal.' : 'Misses the goal.'} {count(SOLVER_PATHS)} lives, the same ones in every test.</TipNote>
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
