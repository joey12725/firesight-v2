// Canvas mark renderers shared by the charts: quantile bands, lines with gaps, the
// density fan and step areas. They draw in CSS pixels on a context from `CanvasLayer`
// and take plain arrays, so any view can feed them from a Summary.
import { clamp, type Scale } from './scales';
import { rampTable } from './theme';

type Series = readonly (number | null | undefined)[];

const ok = (v: number | null | undefined): v is number => v !== null && v !== undefined && Number.isFinite(v);

/** Clips drawing to the plot rectangle until the returned function is called. */
export function clipPlot(ctx: CanvasRenderingContext2D, plot: { x: number; y: number; width: number; height: number }, bleed = 0): () => void {
  ctx.save();
  ctx.beginPath();
  ctx.rect(plot.x - bleed, plot.y - bleed, plot.width + bleed * 2, plot.height + bleed * 2);
  ctx.clip();
  return () => ctx.restore();
}

export interface LineStyle {
  color: string;
  width?: number;
  dash?: number[];
  alpha?: number;
}

/** A polyline through the non-null points; a null breaks the line. */
export function drawLine(ctx: CanvasRenderingContext2D, xs: readonly number[], ys: Series, y: Scale, style: LineStyle) {
  ctx.save();
  ctx.strokeStyle = style.color;
  ctx.lineWidth = style.width ?? 2;
  ctx.lineJoin = 'round';
  ctx.lineCap = 'round';
  ctx.globalAlpha = style.alpha ?? 1;
  if (style.dash) ctx.setLineDash(style.dash);
  ctx.beginPath();
  let pen = false;
  for (let i = 0; i < xs.length; i++) {
    const v = ys[i];
    if (!ok(v)) {
      pen = false;
      continue;
    }
    const px = xs[i] as number;
    const py = y(v);
    if (pen) ctx.lineTo(px, py);
    else ctx.moveTo(px, py);
    pen = true;
  }
  ctx.stroke();
  ctx.restore();
}

/** The area between two series (a quantile band). Runs of non-null pairs are filled separately. */
export function drawBand(ctx: CanvasRenderingContext2D, xs: readonly number[], lower: Series, upper: Series, y: Scale, fill: string) {
  ctx.save();
  ctx.fillStyle = fill;
  let start = -1;
  const flush = (end: number) => {
    if (start < 0 || end - start < 1) return;
    ctx.beginPath();
    for (let i = start; i <= end; i++) {
      const py = y(upper[i] as number);
      if (i === start) ctx.moveTo(xs[i] as number, py);
      else ctx.lineTo(xs[i] as number, py);
    }
    for (let i = end; i >= start; i--) ctx.lineTo(xs[i] as number, y(lower[i] as number));
    ctx.closePath();
    ctx.fill();
  };
  for (let i = 0; i < xs.length; i++) {
    if (ok(lower[i]) && ok(upper[i])) {
      if (start < 0) start = i;
    } else {
      flush(i - 1);
      start = -1;
    }
  }
  flush(xs.length - 1);
  ctx.restore();
}

/** One year of a distribution on the engine's histogram grid, as shares of the living. */
export interface DensityColumn {
  /** Cell boundaries in data units, ascending: `shares.length + 1` of them. */
  edges: ArrayLike<number>;
  /** Share of the living in each cell (sums to at most 1). */
  shares: ArrayLike<number>;
  /** Extra share sitting exactly at zero. */
  zero: number;
  /** 0..1: how much of the picture this column deserves (the share still alive). */
  weight: number;
}

export interface DensityOptions {
  /** Ramp of colour steps, low to high density (theme.baseline.ramp). */
  ramp: string[];
  /** Shapes the response: below 1 lifts faint tails. Default 0.6. */
  gamma?: number;
  /** Strongest opacity. Default 0.92. */
  maxAlpha?: number;
}

/**
 * The density fan: for each year a column of the distribution, shaded by how likely
 * each level is at that age (darker is more likely), blended smoothly between years.
 * Density is measured per pixel of height, so it stays honest on a log axis, and each
 * column is scaled to its own mode, as in a central bank's fan chart: read the colour
 * down a column, not across them. Draws from at most a few thousand numbers whatever
 * the number of simulated lives.
 */
export function drawDensityFan(
  ctx: CanvasRenderingContext2D,
  plot: { x: number; y: number; width: number; height: number },
  xs: readonly number[],
  columns: readonly DensityColumn[],
  y: Scale,
  { ramp, gamma = 0.6, maxAlpha = 0.92 }: DensityOptions,
) {
  const n = columns.length;
  if (n < 2) return;
  const rows = Math.max(8, Math.round(plot.height));
  const table = rampTable(ramp);
  const off = document.createElement('canvas');
  off.width = n;
  off.height = rows;
  const octx = off.getContext('2d');
  if (!octx) return;
  const image = octx.createImageData(n, rows);

  const centre = new Float32Array(4096);
  const level = new Float32Array(4096);
  const halfSpan = new Float32Array(4096);
  const smooth = new Float32Array(rows);

  for (let c = 0; c < n; c++) {
    const col = columns[c] as DensityColumn;
    // Density per pixel of height at each occupied cell's centre, top of the plot first.
    const cells = Math.min(col.shares.length, 4094);
    let m = 0;
    let started = false;
    for (let k = cells - 1; k >= 0; k--) {
      let share = col.shares[k] as number;
      const lo = col.edges[k] as number;
      const hi = col.edges[k + 1] as number;
      if (col.zero > 0 && lo <= 0 && hi > 0) share += col.zero;
      if (!(share > 0) && !started) continue;
      const top = y(hi) - plot.y;
      const bottom = y(lo) - plot.y;
      const span = bottom - top;
      if (!(span > 0)) continue;
      if (!started) {
        // The distribution ends here: density falls to nothing at the cell's far edge.
        centre[m] = top;
        level[m++] = 0;
        started = true;
      }
      centre[m] = (top + bottom) / 2;
      halfSpan[m] = span / 2;
      // Cells thinner than a pixel pile up; never let one read as denser than a pixel can hold.
      level[m++] = share / Math.max(span, 1);
    }
    // Trim trailing empty cells, then close the lower edge.
    while (m > 1 && level[m - 1] === 0) m--;
    if (m > 0) {
      centre[m] = (centre[m - 1] as number) + Math.max(0.5, halfSpan[m - 1] as number);
      level[m++] = 0;
    }
    // Sample the piecewise-linear density at each pixel row.
    let peak = 0;
    let j = 0;
    for (let r = 0; r < rows; r++) {
      const py = r + 0.5;
      while (j < m - 1 && (centre[j + 1] as number) < py) j++;
      let v = 0;
      if (m > 1 && py >= (centre[0] as number) && py <= (centre[m - 1] as number)) {
        const a = centre[j] as number;
        const b = centre[j + 1] as number;
        const f = b > a ? (py - a) / (b - a) : 0;
        v = (level[j] as number) + ((level[j + 1] as number) - (level[j] as number)) * Math.min(1, Math.max(0, f));
      }
      smooth[r] = v;
      if (v > peak) peak = v;
    }
    const fade = 0.3 + 0.7 * Math.sqrt(clamp(col.weight, 0, 1));
    for (let r = 0; r < rows; r++) {
      const t = peak > 0 ? ((smooth[r] as number) / peak) ** gamma : 0;
      const o = (r * n + c) * 4;
      const ti = Math.min(255, Math.round((0.12 + 0.88 * t) * 255)) * 3;
      image.data[o] = table[ti] as number;
      image.data[o + 1] = table[ti + 1] as number;
      image.data[o + 2] = table[ti + 2] as number;
      image.data[o + 3] = t < 0.004 ? 0 : Math.round(255 * maxAlpha * fade * Math.min(1, t * 1.6 + 0.08));
    }
  }
  octx.putImageData(image, 0, 0);

  // Column centres sit at xs[i]; stretch the strip so each texel is centred on its year.
  const x0 = xs[0] as number;
  const x1 = xs[n - 1] as number;
  const step = (x1 - x0) / (n - 1);
  const done = clipPlot(ctx, plot);
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(off, x0 - step / 2, plot.y, step * n, plot.height);
  done();
}

/** Stacked areas from cumulative boundaries: `layers[k][i]` is the top of layer k at position i, in data units. */
export function stackSeries(series: readonly (readonly number[])[]): number[][] {
  const length = series[0]?.length ?? 0;
  const running = new Array<number>(length).fill(0);
  return series.map((s) => s.map((v, i) => (running[i] = (running[i] as number) + (v ?? 0))));
}

/** Horizontal gridlines on the canvas, for charts whose dense marks should sit above the grid. */
export function drawGrid(ctx: CanvasRenderingContext2D, plot: { x: number; y: number; width: number; height: number }, y: Scale, ticks: readonly number[], color: string, zeroColor?: string) {
  ctx.save();
  ctx.lineWidth = 1;
  for (const t of ticks) {
    const py = Math.round(y(t)) + 0.5;
    if (py < plot.y - 0.5 || py > plot.y + plot.height + 0.5) continue;
    ctx.strokeStyle = t === 0 && zeroColor ? zeroColor : color;
    ctx.beginPath();
    ctx.moveTo(plot.x, py);
    ctx.lineTo(plot.x + plot.width, py);
    ctx.stroke();
  }
  ctx.restore();
}
