# The chart kit

Everything a results view needs to draw engine output: a frame with canvas and SVG
layers, scales, axes, a shared crosshair and time range, annotations, animation between
refinements, and a card that gives each chart its words, its table and its exports.
No chart library: marks are drawn by hand on `<canvas>` (dense) or SVG (sparse).

`src/views/overview/` is the reference implementation. Read `NetWorthFan.tsx` (canvas +
SVG + cursor), `StatusStack.tsx` (pure SVG) and `index.tsx` (page composition) before
building a view.

```ts
import { ChartCard, ChartFrame, CanvasLayer, SvgLayer, Cursor, AgeAxis, ValueAxis, ... } from '@/ui/charts';
```

## The rules every chart follows

1. **Form first, colour last.** Decide what the reader must do (compare, see a trend,
   read one number) before anything else. One number is a figure, not a chart.
2. **One y axis.** Never two scales on one plot. A second measure gets its own strip
   under the plot (see the "still alive" strip under the fan) or its own chart.
3. **Every chart sits in a `ChartCard`** with a `takeaway` sentence computed from the
   data, a `caption` with the definitions, and a `table()` (which gives the table view
   and the CSV). A tooltip is never the only way to read a value.
4. **Time runs along the first person's age**, with calendar years as the secondary
   ticks: use `AgeAxis`. Take the x domain from `useTimeRange` so the view's charts zoom
   together, and put a `Cursor` on top so their crosshairs move together.
5. **Money is today's dollars, said once** (the `ValueAxis` title), never on each tick.
6. **Probabilities are counts of lives**: `ofHundred(0.83).text` gives "83 of 100" (and
   "4 of 1,000" for rare outcomes). Show the interval wherever a probability is the point.
7. **Colour does one job per chart** (see Colour). Text is always in ink tokens, never
   in a series colour; a swatch or line key beside it carries identity.
8. **Two or more series need a `Legend`**, plus direct labels (`EndLabels`, or a label
   inside an area when it fits) for up to about four of them.
9. **Marks are quiet**: 2 px lines, bars at most 24 px thick with a 4 px rounded data
   end, a 2 px gap in the surface colour between touching fills, a 2 px surface ring on
   dots, hairline solid gridlines. Dashes are for a target or plan line only.
10. **Nothing moves by itself.** Animation only carries the picture from one result to
    the next (`useAnimated`), and stops under `prefers-reduced-motion`.
11. **Never draw per path.** Charts draw from the Summary (65 years x a few hundred
    numbers), so 250,000 lives cost the same as 2,000. Hover must not redraw a canvas:
    hover marks go in the `Cursor`'s `marks` or an SVG layer.
12. **Aggregate charts lead to a life.** Where a mark is one simulated life, clicking it
    calls `navigateToLife(pathIndex, why)` from `@/state/route`.

## Anatomy of a chart

```tsx
function MedianNetWorth({ summary }: { summary: Summary }) {
  const sync = useChartSync();                                   // the view's shared crosshair + range
  const time = useMemo(() => summaryTime(summary), [summary]);
  const range = useTimeRange(time, sync);                        // { domain, first, last, zoomed, set, reset }
  const target = useMemo(() => quantileSeries(summary, 'net_worth', 0.5), [summary]);
  const median = useAnimated(target, { resetKey: summary.axis.start });   // glides between refinements

  return (
    <ChartFrame height={(w) => Math.min(360, Math.max(240, w * 0.45))} margin={{ left: 52, right: 60 }} aria-label="Median net worth by age">
      {({ plot, theme }) => {
        const x = linearScale(range.domain, [plot.x, plot.right]);
        const index = Array.from({ length: range.last - range.first + 1 }, (_, k) => range.first + k);
        const xs = index.map((i) => x(time.ages[i]!));
        const ys = index.map((i) => median[i] ?? null);
        const y = linearScale([0, Math.max(1, ...ys.map((v) => v ?? 0)) * 1.06], [plot.bottom, plot.y]);
        return (
          <>
            <CanvasLayer deps={[median, range.first, range.last]} draw={(ctx) => drawLine(ctx, xs, ys, y, { color: theme.baseline.median })} />
            <SvgLayer>
              <ValueAxis y={y} format="money" title="Net worth, today's dollars" />
              <AgeAxis x={x} time={time} />
              <EndLabels labels={[{ key: 'p50', y: y(ys.at(-1) ?? 0), text: 'Median', color: theme.baseline.median, strong: true }]} />
            </SvgLayer>
            <Cursor
              count={index.length}
              xOf={(k) => xs[k]!}
              label="Median net worth by age"
              describe={(k) => `Age ${Math.floor(time.ages[index[k]!]!)}: ${moneyCompact(ys[k] ?? 0)}`}
              tooltip={(k) => (<><TipTitle detail={String(time.years[index[k]!])}>Age {Math.floor(time.ages[index[k]!]!)}</TipTitle><TipRow color={theme.baseline.median} label="Median" value={moneyCompact(ys[k] ?? 0)} strong /></>)}
              onBrush={(a, b) => range.set([time.ages[index[a]!]!, time.ages[index[b]!]!])}
            />
          </>
        );
      }}
    </ChartFrame>
  );
}

// In the view:
<ChartSyncProvider>
  <TimeRangeControl time={time} presets={[{ id: 'all', label: 'Whole plan', range: null }]} />
  <ChartCard title="Median net worth" takeaway={sentence} caption="..." table={() => tableData} explain={{ context: () => keyNumbers }} dimmed={run.stale}>
    <MedianNetWorth summary={summary} />
  </ChartCard>
</ChartSyncProvider>
```

Layer order is source order: canvas first, SVG above it, `Cursor` last. Canvas and SVG
share one coordinate system (CSS pixels of the frame); `frame.plot` is the plot
rectangle `{ x, y, width, height, right, bottom }`.

## Reference

### Frame and layers (`ChartFrame.tsx`)

| | |
|---|---|
| `ChartFrame` | Responsive box. `height` (number or `(width) => number`, axis bands included), `margin`, `aria-label` (required). Children: nodes, or a function of the `Frame`. Re-renders on resize and theme change. |
| `useFrame()` | The `Frame`: `width`, `height`, `margin`, `plot`, `theme`. |
| `CanvasLayer` | `draw(ctx, frame)` in CSS pixels on a cleared hi-dpi canvas; redraws when the frame, the theme or `deps` change. Keep hover out of `deps`. |
| `SvgLayer` | SVG over the frame. Give text explicit `fill={theme...}` and `fontSize` (attributes, not classes) so it survives export to PNG. |
| `PlotClip` | Clips SVG children to the plot (use when zoomed lines or areas run past it). On canvas use `clipPlot(ctx, plot)`. |

### Scales (`scales.ts`)

`linearScale(domain, range)`, `symlogScale(domain, range, constant)` (wealth that
crosses zero or spans decades; linear within `constant` of zero) and `niceDomain`.
A `Scale` is a function with `.invert`, `.domain`, `.range`, `.ticks(count)`, `.kind`.
`createTimeAxis({ ages, years })` (or `summaryTime(summary)`) gives the `TimeAxis`:
`ages`, `years`, `domain`, `indexAt(age)`, `yearAtAge(age)`, `ageInYear(year)`.
Per-year arrays are indexed by year index; x positions are `x(time.ages[i])`.

### Axes (`Axis.tsx`)

`AgeAxis { x, time, label?, years? }`: age ticks with calendar years beneath; needs
36 px of bottom margin (22 with `years={false}`). `ValueAxis { y, format, title?, ticks?,
count?, grid?, titleY? }`: gridlines, labels in the left margin (52 px suits money),
`format` is `'money' | 'percent' | 'share100' | 'age' | 'count' | 'ratio'` or a function.
Money ticks share one unit across the axis (`moneyTicks`). When dense marks should sit
above the grid, pass `grid={false}` and call `drawGrid(ctx, plot, y, ticks, theme.grid)`
first in the canvas layer with the same `ticks`. `AgeGrid` draws vertical gridlines.

### Interaction (`Cursor.tsx`, `sync.ts`, `TimeRangeControl.tsx`)

`Cursor` is the whole interaction layer: `count` positions, `xOf(index)` in pixels,
`label` and `describe(index)` for assistive technology, `tooltip(index)`,
`marks(index)` (SVG dots at the focus), `onBrush(from, to)` (drag to zoom),
`onSelect(index, y)` and `onPointer(index, x, y)` (hit-testing your own marks),
`crosshair={false}` for bar and dot charts, `showTooltip="always"` to show the tooltip
when a sibling chart moves the focus. It snaps to the nearest position, so the pointer
never has to land on a line. Keyboard: Tab to the plot, Left/Right (Shift for 5), Home,
End, Escape, Enter to select; each move is announced through an `aria-live` region.
Touch scrubs; vertical scrolling still works.

Build tooltips from `TipTitle`, `TipRow` (line key, label, value in strong ink at the
right) and `TipNote`. List every series at that position, largest first or in mark order.

`ChartSyncProvider` around a view's charts shares one focus and one time range.
`useChartSync()`, `useFocus(sync)`, `useTimeRange(time, sync)`. Outside a provider a
chart gets a private store and still works. `TimeRangeControl { time, presets }` is the
view's range filter: one row above the charts it scopes, never inside a card.

### Annotations (`annotations.tsx`) and legend (`Legend.tsx`)

`RuleAnnotations { items: [{ x, label, detail?, tone? }] }`: labelled vertical rules
with labels laid out in rows in the top margin (`rulesHeight(n)` says how much).
`planMoments(summary, profile)` returns the standard ones: work optional (median),
retirement, Social Security starts, mortgage paid. They are the engine's own
`summary.plan_moments` (medians over the lives in which each thing happens, marked only
when it happens in at least half of lives); the profile supplies names and puts a
partner's age on the first person's axis. Do not re-derive them from the profile. `EventMarker { x, y, label }`: a
ringed dot with a short label. `EndLabels { labels }`: direct labels at line ends in the
right margin, spread apart with leader lines when they would collide (`spreadLabels`).
`textWidth(text, size)` estimates width for fit tests. `Legend { items }` with
`swatch: 'box' | 'line' | 'dash' | 'dot' | 'ring'`; wrap it in `<div data-chart-legend>`
inside the card so it is drawn into the exported PNG.

### Marks (`marks.ts`)

`drawLine(ctx, xs, ys, y, { color, width, dash, alpha })` (null breaks the line),
`drawBand(ctx, xs, lower, upper, y, fill)`, `drawGrid`, `clipPlot`, `stackSeries(series)`
(cumulative tops for stacked areas) and `drawDensityFan(ctx, plot, xs, columns, y,
{ ramp })`, which shades a yearly distribution from the engine's histogram grid.
Build its input with `fanModel(summary, series)` and `fanColumns(model, first, last)`;
animate it with `useAnimated(model, { blend: interpolateFan })`.

### Summary selectors (`summary.ts`)

`summaryTime`, `quantileSeries(summary, series, level)`, `compositionSeries(summary,
key)`, `statusShares(summary, status)`, `aliveShare`, `indexAtAge`, `QUANTILE_LEVELS`,
`fanModel`, `planMoments`. Use them instead of indexing arrays by position. The planned-spending
line is `quantileSeries(summary, 'spend_planned', 0.5)`; the named lives of the fan come from
`summary.pick_paths` (see `namedLives` in `views/overview/NetWorthFan.tsx`), not from traces.

### Animation (`animate.tsx`)

`useAnimated(target, { duration, resetKey, blend })` returns a value that eases to each
new `target` (numbers, arrays and plain objects blend structurally; a null or a changed
length snaps). Memoise `target`. Pass a `resetKey` that changes with the plan's shape
(start month, number of years) so a different plan snaps instead of morphing.
`AnimatedNumber { value, format }` tweens a headline figure without re-rendering.
`interpolate(a, b, t)` is the structural blend. All of it respects reduced motion.

### The card (`ChartCard.tsx`, `export.ts`)

`ChartCard { title, takeaway, caption, controls, table, filename, explain, dimmed }`.
`controls` are for this chart's encoding only (linear/log, which series).
`table: () => TableData` (`{ caption, columns: [{ key, label, format? }], rows }`) feeds
the table view and the CSV (raw values). The image button copies a 2x PNG of every
`[data-chart-layer]` in the card with the title, takeaway and caption (pass
`takeawayText` / `captionText` when those are not plain strings). `explain: { context:
() => keyNumbers }` adds "Explain this chart", which calls `askAssistant(prompt,
{ chart, takeaway, ...context })`: give it a few hundred bytes of the numbers on
screen, not the Summary. `dimmed={run.stale}` holds the old picture at reduced strength
while a new result is on its way; never swap a chart for a skeleton on a re-run.
`DataTable` renders a `TableData` on its own.

### Formatting (`format.ts`)

`ofHundred(share)`, `age(v)`, `percentTick`, `moneyTicks(ticks)`, `formatValue(v,
format)`, `csvValue`. `money`, `moneyCompact` and `count` come from `@/ui`.

## Colour

Tokens live in `src/styles/tokens.css`; read them through `useChartTheme()` (or
`frame.theme`). `palette.test.ts` checks every palette in both themes for lightness,
chroma, colour-blind separation of neighbours and contrast; if you change a token, it
must still pass.

| Job | Use | Tokens |
|---|---|---|
| The plan (baseline) | lines, bands, dots of the plan as entered | `theme.baseline` (`line`, `median`, `bands[outer, mid, inner]`, `samplePath`, `ramp`) |
| A scenario | the same marks for a compared scenario | `theme.scenario` |
| Magnitude | density, heat maps: one hue, more is stronger | `theme.sequential` (ember) and `theme.sequentialCool` (blue), 8 steps; `rampCss(ramp, t)`, `rampTable(ramp)`. Discrete ordered marks start at step 4. |
| Life stage | the six statuses, always in `STATUS_STACK` order with `STATUS_LABEL` / `STATUS_SHORT` | `theme.status[status]` |
| Identity | composition (accounts, income sources, spending groups) | `theme.categorical[0..6]` assigned in order, never cycled or re-assigned on filter; `[7]` is "Other" |
| Good / bad | only when the colour means it, with an icon or a word | `theme.semantic` |

A series that means trouble (lives short of essentials, a spending cut) wears the status
colour, not a categorical one. More than seven categories: fold the tail into "Other"
or use small multiples. `inkOn(fill)` picks white or near-black for a label inside a
fill; `withAlpha(color, a)` for washes.

## Progressive results

`summary` refines several times a second while `run.status === 'running'`. Draw what
you have: `summary.paths` says how many lives are in it. `run.stale` means the summary
still describes the plan before the latest edit: pass it to `dimmed`. A view renders a
skeleton only when `summary` is null. Small samples deserve a visible warning (see
`Sharpness` in the Overview).

## Charts for the assistant

Register a compact variant at startup in `src/views/assistant-charts.ts`:
`registerAssistantChart('fan', () => import('./overview/compact/Fan'))`. The default
export takes `AssistantChartProps`, renders at about 320 px wide and 160 to 220 px tall
with no card, no toolbar and direct labels, and returns null when `summary` is null.
Most chart components here take a `compact` prop for exactly this.
