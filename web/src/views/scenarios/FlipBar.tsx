// Rescued, harmed, unchanged: every simulated life in one bar, in four groups. The
// lives that fall short either way sit at the left and those that hold either way at
// the right, so the lives the decision flips are at the boundary between them: the
// change moves that boundary.
import { count } from '@/ui';
import { ChartFrame, Cursor, Legend, SvgLayer, TipNote, TipRow, TipTitle, ofHundred, useChartTheme, withAlpha, type ChartTheme, type TableData } from '@/ui/charts';
import type { Flips } from './model';

interface Part {
  key: 'neither' | 'rescued' | 'harmed' | 'both';
  label: string;
  short: string;
  note: string;
  lives: number;
  color: string;
}

function parts(f: Flips, theme: ChartTheme): Part[] {
  return [
    { key: 'neither', label: 'Falls short either way', short: 'Short either way', note: 'The plan falls short under both decisions.', lives: f.neither, color: withAlpha(theme.ink.secondary, 0.62) },
    { key: 'rescued', label: 'Rescued by the change', short: 'Rescued', note: 'Falls short under the plan, holds with the change.', lives: f.rescued, color: theme.semantic.good },
    { key: 'harmed', label: 'Harmed by the change', short: 'Harmed', note: 'Holds under the plan, falls short with the change.', lives: f.harmed, color: theme.semantic.bad },
    { key: 'both', label: 'Holds either way', short: 'Holds either way', note: 'The plan holds under both decisions.', lives: f.both, color: withAlpha(theme.ink.tertiary, theme.dark ? 0.5 : 0.3) },
  ];
}

export function flipTable(f: Flips): TableData {
  const rows: [string, number][] = [
    ['Falls short under both', f.neither],
    ['Rescued: falls short under the plan, holds with the change', f.rescued],
    ['Harmed: holds under the plan, falls short with the change', f.harmed],
    ['Holds under both', f.both],
  ];
  return {
    caption: 'Simulated lives by whether the plan holds under the baseline and under the scenario',
    columns: [
      { key: 'group', label: 'Group', align: 'left' },
      { key: 'lives', label: 'Lives' },
      { key: 'share', label: 'Share of lives', format: (v) => `${(v * 100).toFixed(2)}%` },
    ],
    rows: [
      ...rows.map(([group, lives]) => ({ group, lives, share: f.paths ? lives / f.paths : 0 })),
      { group: 'Net change in lives in which the plan holds', lives: f.rescued - f.harmed, share: f.net.value },
      { group: '95% interval of the net change, low', lives: null, share: f.net.lo },
      { group: '95% interval of the net change, high', lives: null, share: f.net.hi },
    ],
  };
}

export function FlipLegend({ flips, className }: { flips: Flips; className?: string }) {
  const theme = useChartTheme();
  return (
    <div data-chart-legend>
      <Legend className={className} items={parts(flips, theme).map((p) => ({ key: p.key, label: p.label, color: p.color, detail: count(p.lives) }))} />
    </div>
  );
}

/** The four groups as one bar. Flipped groups are never drawn thinner than 3 px, so a rare rescue is still seen. */
export function FlipBar({ flips, compact }: { flips: Flips; compact?: boolean }) {
  const total = Math.max(1, flips.paths);
  return (
    <ChartFrame height={compact ? 46 : 64} margin={{ top: compact ? 18 : 26, right: 0, bottom: compact ? 4 : 14, left: 0 }} aria-label="Simulated lives by whether the plan holds under each decision">
      {({ plot, theme }) => {
        const all = parts(flips, theme);
        const shown = all.filter((p) => p.lives > 0);
        const gap = 2;
        const room = plot.width - gap * Math.max(0, shown.length - 1);
        // Proportional widths with a floor, taken from the widest group so the total still fits.
        const raw = shown.map((p) => Math.max(p.key === 'rescued' || p.key === 'harmed' ? 3 : 1, (p.lives / total) * room));
        const over = raw.reduce((a, b) => a + b, 0) - room;
        const widest = raw.indexOf(Math.max(...raw));
        const widths = raw.map((w, i) => (i === widest ? w - over : w));
        let cursor = plot.x;
        const boxes = shown.map((p, i) => {
          const box = { part: p, x: cursor, width: widths[i] as number };
          cursor += box.width + gap;
          return box;
        });
        const flipped = boxes.filter((b) => b.part.key === 'rescued' || b.part.key === 'harmed');
        const centre = flipped.length ? ((flipped[0]?.x ?? 0) + (flipped[flipped.length - 1]?.x ?? 0) + (flipped[flipped.length - 1]?.width ?? 0)) / 2 : null;
        const flippedLives = flips.rescued + flips.harmed;
        const text = flippedLives > 0 ? `${count(flippedLives)} ${flippedLives === 1 ? 'life flips' : 'lives flip'}: ${count(flips.rescued)} rescued, ${count(flips.harmed)} harmed` : 'No life flips';
        const anchor = centre === null ? 'middle' : centre < plot.x + 120 ? 'start' : centre > plot.right - 120 ? 'end' : 'middle';
        const labelX = centre === null ? (plot.x + plot.right) / 2 : anchor === 'start' ? Math.max(plot.x, centre - 12) : anchor === 'end' ? Math.min(plot.right, centre + 12) : centre;
        return (
          <>
            <SvgLayer>
              {boxes.map((b, i) => {
                const first = i === 0;
                const last = i === boxes.length - 1;
                const r = Math.min(4, b.width / 2);
                const { x, width } = b;
                const y = plot.y;
                const h = plot.height;
                // Rounded only at the two ends of the whole bar.
                const d = `M${x + (first ? r : 0)},${y} H${x + width - (last ? r : 0)} ${last ? `Q${x + width},${y} ${x + width},${y + r} V${y + h - r} Q${x + width},${y + h} ${x + width - r},${y + h}` : `V${y + h}`} H${x + (first ? r : 0)} ${first ? `Q${x},${y + h} ${x},${y + h - r} V${y + r} Q${x},${y} ${x + r},${y}` : `V${y}`} Z`;
                return <path key={b.part.key} d={d} fill={b.part.color} />;
              })}
              <g fontSize={11}>
                {centre !== null && <line x1={centre} x2={centre} y1={plot.y - 5} y2={plot.y - 1} stroke={theme.ink.secondary} strokeWidth={1} />}
                <text x={labelX} y={plot.y - 9} textAnchor={anchor} fill={theme.ink.primary} fontWeight={500}>
                  {text}
                </text>
                {!compact && (
                  <>
                    <text x={plot.x} y={plot.bottom + 12} fill={theme.label}>
                      {flips.neither > 0 ? 'Falls short either way' : ''}
                    </text>
                    <text x={plot.right} y={plot.bottom + 12} textAnchor="end" fill={theme.label}>
                      {flips.both > 0 ? 'Holds either way' : ''}
                    </text>
                  </>
                )}
              </g>
            </SvgLayer>
            <Cursor
              count={boxes.length}
              xOf={(i) => (boxes[i]?.x ?? 0) + (boxes[i]?.width ?? 0) / 2}
              label="Lives by whether the plan holds under each decision"
              crosshair={false}
              describe={(i) => {
                const p = boxes[i]?.part;
                return p ? `${p.label}: ${count(p.lives)} of ${count(flips.paths)} lives. ${p.note}` : '';
              }}
              marks={(i) => {
                const b = boxes[i];
                return b ? <rect x={b.x - 1.5} y={plot.y - 1.5} width={b.width + 3} height={plot.height + 3} rx={3} fill="none" stroke={theme.ink.primary} strokeWidth={1.5} /> : null;
              }}
              tooltip={(i) => {
                const p = boxes[i]?.part;
                if (!p) return null;
                return (
                  <>
                    <TipTitle>{p.label}</TipTitle>
                    <TipRow color={p.color} swatch="box" label="Lives" value={`${count(p.lives)} of ${count(flips.paths)}`} strong />
                    <TipRow label="As a share" value={ofHundred(p.lives / total).text} />
                    <TipNote>{p.note}</TipNote>
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
