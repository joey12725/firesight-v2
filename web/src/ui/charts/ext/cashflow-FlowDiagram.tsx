// A two-sided flow diagram (a Sankey with one waist): sources on the left, uses on the
// right, every ribbon as thick as its amount, all of it passing through one trunk in
// the middle. The trunk is deliberate: the data says how much came from each source
// and how much went to each use, not which dollar paid for what, so no source is tied
// to a particular use. Laid out by hand: nodes keep a fixed order, labels are spread
// apart so they never collide, and values glide when the year changes.
import { useMemo, useState } from 'react';
import { ChartFrame, SvgLayer, spreadLabels, textWidth, useAnimated, withAlpha } from '@/ui/charts';

export interface FlowNode {
  key: string;
  label: string;
  /** Nodes of one group sit together, with a wider gap to the next group. */
  group: string;
  color: string;
  value: number;
}

export interface FlowLayoutOptions {
  width: number;
  height: number;
  /** Room for labels at each side. */
  labelWidth: number;
  top?: number;
  nodeWidth?: number;
  gap?: number;
  groupGap?: number;
  /** Smallest distance between two label baselines. */
  labelGap?: number;
  /** Nodes under this share of the total are drawn but not labelled. */
  minLabelShare?: number;
}

export interface PlacedNode {
  key: string;
  /** Left edge of the node bar. */
  x: number;
  y: number;
  height: number;
  /** The ribbon between this node and its segment of the trunk. */
  ribbon: string;
  /** Baseline centre of the label, or null when the node is too small to label. */
  labelY: number | null;
  /** True when the label had to move away from the node's centre (draw a leader). */
  moved: boolean;
}

export interface FlowLayout {
  /** Pixels per unit of value. */
  scale: number;
  total: number;
  sources: PlacedNode[];
  uses: PlacedNode[];
  trunk: { x: number; y: number; width: number; height: number };
}

/** Places both sides and the trunk. Pure, so it can be tested without a DOM. */
export function layoutFlow(sources: readonly FlowNode[], uses: readonly FlowNode[], options: FlowLayoutOptions): FlowLayout {
  const { width, height, labelWidth, top = 0, nodeWidth = 8, gap = 4, groupGap = 12, labelGap = 15, minLabelShare = 0.004 } = options;
  const sum = (nodes: readonly FlowNode[]) => nodes.reduce((t, n) => t + Math.max(0, n.value), 0);
  const total = Math.max(sum(sources), sum(uses));
  const padding = (nodes: readonly FlowNode[], scale: number) => {
    let pad = 0;
    let previous: FlowNode | null = null;
    for (const n of nodes) {
      if (!(n.value > 0)) continue;
      // A gap fades in with the node, so a node appearing mid-animation does not make the column jump.
      if (previous) pad += Math.min(previous.group === n.group ? gap : groupGap, n.value * scale * 2 + 1);
      previous = n;
    }
    return pad;
  };
  // Two passes: the gaps depend (slightly) on the scale and the scale on the gaps.
  let scale = total > 0 ? (height - Math.max(sources.length, uses.length) * gap) / total : 0;
  for (let pass = 0; pass < 2; pass++) scale = total > 0 ? Math.max(0, height - Math.max(padding(sources, scale), padding(uses, scale))) / total : 0;

  const trunkWidth = nodeWidth + 2;
  const trunkHeight = total * scale;
  const trunk = { x: width / 2 - trunkWidth / 2, y: top + (height - trunkHeight) / 2, width: trunkWidth, height: trunkHeight };

  const place = (nodes: readonly FlowNode[], side: 'left' | 'right'): PlacedNode[] => {
    const sideHeight = sum(nodes) * scale + padding(nodes, scale);
    const x = side === 'left' ? labelWidth : width - labelWidth - nodeWidth;
    let y = top + (height - sideHeight) / 2;
    let cy = trunk.y + (trunkHeight - sum(nodes) * scale) / 2;
    let previous: FlowNode | null = null;
    const placed = nodes.map((n) => {
      const h = Math.max(0, n.value) * scale;
      if (h > 0 && previous) y += Math.min(previous.group === n.group ? gap : groupGap, h * 2 + 1);
      if (h > 0) previous = n;
      const x0 = side === 'left' ? x + nodeWidth : x;
      const x1 = side === 'left' ? trunk.x : trunk.x + trunk.width;
      const xm = (x0 + x1) / 2;
      const f = (v: number) => v.toFixed(1);
      const ribbon = h > 0 ? `M${f(x0)},${f(y)}C${f(xm)},${f(y)} ${f(xm)},${f(cy)} ${f(x1)},${f(cy)}L${f(x1)},${f(cy + h)}C${f(xm)},${f(cy + h)} ${f(xm)},${f(y + h)} ${f(x0)},${f(y + h)}Z` : '';
      const out = { key: n.key, x, y, height: h, ribbon, labelY: null as number | null, moved: false, labelled: total > 0 && n.value / total >= minLabelShare };
      y += h;
      cy += h;
      return out;
    });
    const labelled = placed.filter((p) => p.labelled);
    const ys = spreadLabels(labelled.map((p) => p.y + p.height / 2), labelGap, top + 5, top + height - 3);
    labelled.forEach((p, i) => {
      p.labelY = ys[i] as number;
      p.moved = Math.abs((ys[i] as number) - (p.y + p.height / 2)) > 3;
    });
    return placed.map(({ labelled: _labelled, ...rest }) => rest);
  };

  return { scale, total, sources: place(sources, 'left'), uses: place(uses, 'right'), trunk };
}

export interface FlowDiagramProps {
  sources: FlowNode[];
  uses: FlowNode[];
  format: (value: number) => string;
  /** Written over the trunk: "$235k a year". */
  trunkLabel?: (total: number) => string;
  /** Names of the two columns, written above them. */
  headings?: { sources: string; uses: string };
  height?: number | ((width: number) => number);
  compact?: boolean;
  /** Accessible name. */
  label: string;
}

export function FlowDiagram({ sources, uses, format, trunkLabel, headings, height, compact, label }: FlowDiagramProps) {
  const target = useMemo(() => ({ s: sources.map((n) => n.value), u: uses.map((n) => n.value) }), [sources, uses]);
  // Year changes glide; a different set of nodes (another plan) snaps.
  const values = useAnimated(target, { resetKey: `${sources.map((n) => n.key).join(',')}|${uses.map((n) => n.key).join(',')}`, duration: 420 });
  const [active, setActive] = useState<{ key: string; x: number; y: number } | null>(null);
  const size = compact ? 10 : 11;
  const text = (n: FlowNode) => textWidth(n.label, size) + textWidth(format(n.value), size) * 1.08 + 8;

  return (
    <ChartFrame height={height ?? ((w) => (compact ? 230 : Math.round(Math.min(460, Math.max(340, w * 0.42)))))} margin={{ top: 0, right: 0, bottom: 0, left: 0 }} aria-label={label}>
      {(frame) => {
        const { theme, width } = frame;
        const widest = Math.max(60, ...sources.filter((n) => n.value > 0).map(text), ...uses.filter((n) => n.value > 0).map(text));
        const labelWidth = Math.min(widest + 12, Math.max(84, width * 0.36));
        const top = headings || trunkLabel ? 24 : 4;
        const animated = (nodes: FlowNode[], row: number[]) => nodes.map((n, i) => ({ ...n, value: row[i] ?? n.value }));
        const layout = layoutFlow(animated(sources, values.s), animated(uses, values.u), { width, height: frame.height - top - 6, top, labelWidth, labelGap: compact ? 13 : 15 });
        const total = Math.max(
          sources.reduce((t, n) => t + n.value, 0),
          uses.reduce((t, n) => t + n.value, 0),
        );
        const fits = (n: FlowNode) => text(n) <= labelWidth - 6;

        const side = (nodes: FlowNode[], placed: PlacedNode[], where: 'left' | 'right') =>
          nodes.map((n, i) => {
            const p = placed[i] as PlacedNode;
            if (p.height <= 0) return null;
            const dim = active !== null && active.key !== n.key;
            const lx = where === 'left' ? p.x - 8 : p.x + 8 + 8;
            const mid = p.y + p.height / 2;
            return (
              <g
                key={n.key}
                opacity={dim ? 0.35 : 1}
                style={{ transition: 'opacity 120ms', pointerEvents: 'auto' }}
                onPointerEnter={(e) => setActive({ key: n.key, x: e.nativeEvent.offsetX, y: e.nativeEvent.offsetY })}
                onPointerMove={(e) => setActive({ key: n.key, x: e.nativeEvent.offsetX, y: e.nativeEvent.offsetY })}
                onPointerLeave={() => setActive(null)}
              >
                <path d={p.ribbon} fill={withAlpha(n.color, active?.key === n.key ? 0.55 : theme.dark ? 0.34 : 0.28)} />
                <rect x={p.x} y={p.y} width={8} height={Math.max(1, p.height)} rx={Math.min(2, p.height / 2)} fill={n.color} />
                {p.labelY !== null && (
                  <>
                    {p.moved && <path d={`M${where === 'left' ? p.x - 1 : p.x + 9},${mid.toFixed(1)}L${where === 'left' ? p.x - 6 : p.x + 14},${p.labelY.toFixed(1)}`} stroke={theme.axis} strokeWidth={1} fill="none" />}
                    {/* A wide transparent plate so the label is part of the hover target. */}
                    <rect x={where === 'left' ? 0 : p.x + 8} y={p.labelY - 7} width={labelWidth} height={14} fill="transparent" />
                    <text x={lx} y={p.labelY + 3.5} textAnchor={where === 'left' ? 'end' : 'start'} fontSize={size} fill={theme.ink.secondary}>
                      {where === 'left' ? (
                        <>
                          {fits(n) ? `${n.label} ` : ''}
                          <tspan fill={theme.ink.primary} fontWeight={600} style={{ fontVariantNumeric: 'tabular-nums' }}>
                            {format(n.value)}
                          </tspan>
                        </>
                      ) : (
                        <>
                          <tspan fill={theme.ink.primary} fontWeight={600} style={{ fontVariantNumeric: 'tabular-nums' }}>
                            {format(n.value)}
                          </tspan>
                          {fits(n) ? ` ${n.label}` : ''}
                        </>
                      )}
                    </text>
                  </>
                )}
              </g>
            );
          });

        const hovered = active ? [...sources, ...uses].find((n) => n.key === active.key) : undefined;
        return (
          <>
            <SvgLayer style={{ pointerEvents: 'none' }}>
              {side(sources, layout.sources, 'left')}
              {side(uses, layout.uses, 'right')}
              <rect x={layout.trunk.x} y={layout.trunk.y} width={layout.trunk.width} height={Math.max(1, layout.trunk.height)} rx={2} fill={theme.ink.primary} />
              <g fontSize={11} fill={theme.label}>
                {headings && (
                  <>
                    <text x={labelWidth + 8} y={11} textAnchor="end">
                      {headings.sources}
                    </text>
                    <text x={width - labelWidth - 8} y={11} textAnchor="start">
                      {headings.uses}
                    </text>
                  </>
                )}
                {trunkLabel && (
                  <text x={width / 2} y={11} textAnchor="middle" fill={theme.ink.primary} fontWeight={600} style={{ fontVariantNumeric: 'tabular-nums' }}>
                    {trunkLabel(total)}
                  </text>
                )}
              </g>
            </SvgLayer>
            {hovered && active && total > 0 && (
              <div
                role="presentation"
                className="pointer-events-none absolute z-10 rounded-md border border-line bg-surface-2 px-2.5 py-1.5 text-xs whitespace-nowrap shadow-overlay"
                style={{ left: Math.min(Math.max(0, active.x + 12), Math.max(0, width - 170)), top: Math.max(0, active.y - 44) }}
              >
                <div className="flex items-center gap-2">
                  <span aria-hidden className="inline-block size-2 shrink-0 rounded-[2px]" style={{ background: hovered.color }} />
                  <span className="text-ink-2">{hovered.label}</span>
                  <span className="num ml-auto pl-3 font-semibold text-ink-1">{format(hovered.value)}</span>
                </div>
                <div className="mt-0.5 text-2xs text-ink-3">{Math.round((hovered.value / total) * 100)} of every 100 dollars</div>
              </div>
            )}
          </>
        );
      }}
    </ChartFrame>
  );
}
