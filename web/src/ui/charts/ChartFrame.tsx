// The frame every chart is built in: a responsive box that measures itself, exposes
// its plot rectangle and the resolved theme through context, and stacks layers
// (canvas for dense marks, SVG for axes, labels and interaction) in source order.
import { createContext, useContext, useEffect, useId, useMemo, useRef, type CSSProperties, type ReactNode, type SVGProps } from 'react';
import { setupCanvas } from './canvas';
import { useChartTheme, type ChartTheme } from './theme';
import { useResizeObserver } from './useResizeObserver';

export interface Margin {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export interface Frame {
  /** Outer size in CSS pixels. */
  width: number;
  height: number;
  margin: Margin;
  /** The plot rectangle, in the frame's coordinates (canvas and SVG layers share them). */
  plot: { x: number; y: number; width: number; height: number; right: number; bottom: number };
  theme: ChartTheme;
}

const FrameContext = createContext<Frame | null>(null);

/** The enclosing chart frame. Throws outside a `ChartFrame`. */
export function useFrame(): Frame {
  const frame = useContext(FrameContext);
  if (!frame) throw new Error('useFrame must be used inside a ChartFrame');
  return frame;
}

export interface ChartFrameProps {
  /** Height in CSS pixels, axis bands included. A function receives the measured width. */
  height: number | ((width: number) => number);
  margin?: Partial<Margin>;
  /** What the chart shows, for assistive technology. Details belong in the table view. */
  'aria-label': string;
  className?: string;
  style?: CSSProperties;
  /** Layers, bottom first. A function receives the frame (also available from `useFrame`). */
  children: ReactNode | ((frame: Frame) => ReactNode);
}

const DEFAULT_MARGIN: Margin = { top: 12, right: 12, bottom: 36, left: 52 };

export function ChartFrame({ height, margin, className, style, children, ...aria }: ChartFrameProps) {
  const ref = useRef<HTMLDivElement>(null);
  const { width } = useResizeObserver(ref);
  const theme = useChartTheme();
  const w = Math.round(width);
  const h = Math.round(typeof height === 'function' ? height(w) : height);
  const { top, right, bottom, left } = { ...DEFAULT_MARGIN, ...margin };
  const frame = useMemo<Frame>(() => {
    const pw = Math.max(1, w - left - right);
    const ph = Math.max(1, h - top - bottom);
    return {
      width: w,
      height: h,
      margin: { top, right, bottom, left },
      plot: { x: left, y: top, width: pw, height: ph, right: left + pw, bottom: top + ph },
      theme,
    };
  }, [w, h, top, right, bottom, left, theme]);

  return (
    <div ref={ref} role="group" {...aria} data-chart-frame className={className} style={{ position: 'relative', height: h, ...style }}>
      {w > 0 && <FrameContext.Provider value={frame}>{typeof children === 'function' ? children(frame) : children}</FrameContext.Provider>}
    </div>
  );
}

export interface CanvasLayerProps {
  /** Draws in CSS pixels on a cleared canvas. Runs again when the frame, the theme or `deps` change. */
  draw: (ctx: CanvasRenderingContext2D, frame: Frame) => void;
  deps: readonly unknown[];
}

/** A hi-dpi canvas covering the frame. Never put hover state in `deps`: hover belongs in an SVG layer above. */
export function CanvasLayer({ draw, deps }: CanvasLayerProps) {
  const frame = useFrame();
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = setupCanvas(canvas, frame.width, frame.height);
    if (ctx) draw(ctx, frame);
    // `draw` is a fresh closure every render; `deps` says when its output changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [frame, ...deps]);
  return <canvas ref={ref} data-chart-layer aria-hidden style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }} />;
}

/**
 * An SVG covering the frame, in the same coordinates as the canvas. Give text explicit
 * `fill` and `fontSize` from the theme (not classes) so the chart exports as an image.
 */
export function SvgLayer({ children, style, ...rest }: SVGProps<SVGSVGElement>) {
  const frame = useFrame();
  return (
    <svg
      data-chart-layer
      aria-hidden
      width={frame.width}
      height={frame.height}
      viewBox={`0 0 ${frame.width} ${frame.height}`}
      fontFamily={frame.theme.fontSans}
      {...rest}
      style={{ position: 'absolute', inset: 0, overflow: 'visible', pointerEvents: 'none', ...style }}
    >
      {children}
    </svg>
  );
}

/** Clips its SVG children to the plot rectangle (lines and areas that run past the visible time range). */
export function PlotClip({ children, bleed = 0 }: { children: ReactNode; bleed?: number }) {
  const { plot } = useFrame();
  const id = `clip-${useId().replace(/[^a-zA-Z0-9]/g, '')}`;
  return (
    <>
      <defs>
        <clipPath id={id}>
          <rect x={plot.x - bleed} y={plot.y - bleed} width={plot.width + bleed * 2} height={plot.height + bleed * 2} />
        </clipPath>
      </defs>
      <g clipPath={`url(#${id})`}>{children}</g>
    </>
  );
}
