// Theme-aware colour access for chart code. Canvas cannot resolve `var(--x)`, and SVG
// that is exported as an image cannot either, so charts read the resolved token values
// through this hook and redraw when the theme changes.
import { useSyncExternalStore } from 'react';
import type { Status } from '@/engine/output-types';

/** Every chart token in tokens.css, resolved to a CSS colour string. */
export interface ChartTheme {
  dark: boolean;
  fontSans: string;
  fontMono: string;
  /** The surface charts sit on (the page). Use it for the 2px gaps and rings between marks. */
  surface: string;
  /** A raised surface, for tooltips and label plates. */
  raised: string;
  ink: { primary: string; secondary: string; tertiary: string };
  grid: string;
  axis: string;
  /** Axis tick labels. */
  label: string;
  annotation: string;
  /** The plan as entered. `bands` runs outer (5-95) to inner (25-75); `ramp` is its sequential ramp, low to high. */
  baseline: { line: string; median: string; bands: [string, string, string]; samplePath: string; ramp: string[] };
  /** A scenario under comparison. */
  scenario: { line: string; bands: [string, string, string]; ramp: string[] };
  /** Life stages. Stack them in `STATUS_STACK` order. */
  status: Record<Status, string>;
  /** Categorical series colours, in the order to assign them (never cycle; the 8th is "Other"). */
  categorical: string[];
  /** Sequential single-hue ramp (8 steps), low to high: ember. */
  sequential: string[];
  /** A second sequential ramp for when two share a view: blue. */
  sequentialCool: string[];
  semantic: { good: string; warn: string; bad: string; accent: string; cool: string };
}

/** Stacking order of the life stages, from the baseline up: trouble sits on the baseline where its height can be read. */
export const STATUS_STACK: Status[] = ['depleted', 'retired_cutting', 'retired_on_plan', 'retired_surplus', 'working', 'dead'];

export const STATUS_LABEL: Record<Status, string> = {
  working: 'Working',
  retired_surplus: 'Retired, wealth doubled',
  retired_on_plan: 'Retired, on plan',
  retired_cutting: 'Retired, cutting back',
  depleted: 'Short of essentials',
  dead: 'Died',
};

/** Short forms for labels inside narrow areas and compact charts. */
export const STATUS_SHORT: Record<Status, string> = {
  working: 'Working',
  retired_surplus: 'Surplus',
  retired_on_plan: 'On plan',
  retired_cutting: 'Cutting back',
  depleted: 'Short',
  dead: 'Died',
};

function read(): ChartTheme {
  const style = getComputedStyle(document.documentElement);
  const v = (name: string) => style.getPropertyValue(name).trim();
  const steps = (prefix: string) => [1, 2, 3, 4, 5, 6, 7, 8].map((i) => v(`${prefix}-${i}`));
  const sequential = steps('--chart-seq');
  const sequentialCool = steps('--chart-seq-cool');
  return {
    dark: document.documentElement.dataset.theme === 'dark',
    fontSans: v('--ff-sans'),
    fontMono: v('--ff-mono'),
    surface: v('--surface-0'),
    raised: v('--surface-2'),
    ink: { primary: v('--ink-1'), secondary: v('--ink-2'), tertiary: v('--ink-3') },
    grid: v('--chart-grid'),
    axis: v('--chart-axis'),
    label: v('--chart-label'),
    annotation: v('--chart-annotation'),
    baseline: {
      line: v('--chart-baseline'),
      median: v('--chart-median'),
      bands: [v('--chart-band-outer'), v('--chart-band-mid'), v('--chart-band-inner')],
      samplePath: v('--chart-sample-path'),
      ramp: sequential,
    },
    scenario: {
      line: v('--chart-scenario'),
      bands: [v('--chart-scenario-band-outer'), v('--chart-scenario-band-mid'), v('--chart-scenario-band-inner')],
      ramp: sequentialCool,
    },
    status: {
      working: v('--chart-status-working'),
      retired_surplus: v('--chart-status-surplus'),
      retired_on_plan: v('--chart-status-on-plan'),
      retired_cutting: v('--chart-status-cutting'),
      depleted: v('--chart-status-depleted'),
      dead: v('--chart-status-dead'),
    },
    categorical: steps('--chart-cat'),
    sequential,
    sequentialCool,
    semantic: { good: v('--good'), warn: v('--warn'), bad: v('--bad'), accent: v('--accent'), cool: v('--cool') },
  };
}

let cached: ChartTheme | null = null;

function subscribe(onChange: () => void) {
  const handler = () => {
    cached = null;
    onChange();
  };
  window.addEventListener('firesight:theme', handler);
  return () => window.removeEventListener('firesight:theme', handler);
}

/** The current chart theme. For use outside React (for example in a draw function). */
export function getChartTheme(): ChartTheme {
  return (cached ??= read());
}

/** The current chart theme; the component re-renders when light/dark changes. */
export function useChartTheme(): ChartTheme {
  return useSyncExternalStore(subscribe, getChartTheme);
}

// ---- colour arithmetic -------------------------------------------------------------

export type Rgb = [number, number, number];

/** Parses "#rgb", "#rrggbb" or "rgb(r g b / a)" / "rgb(r, g, b)" into 0-255 channels. */
export function parseColor(color: string): Rgb {
  const c = color.trim();
  if (c.startsWith('#')) {
    const h = c.length === 4 ? [...c.slice(1)].map((x) => x + x).join('') : c.slice(1, 7);
    return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
  }
  const m = c.match(/[\d.]+/g);
  return m && m.length >= 3 ? [Number(m[0]), Number(m[1]), Number(m[2])] : [0, 0, 0];
}

export function withAlpha(color: string, alpha: number): string {
  const [r, g, b] = parseColor(color);
  return `rgb(${r} ${g} ${b} / ${alpha})`;
}

/** WCAG relative luminance, 0 (black) to 1 (white). */
export function luminance(color: string): number {
  const [r, g, b] = parseColor(color).map((v) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  }) as Rgb;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** Text colour for a label set inside a coloured fill: white or near-black, by the fill's luminance. */
export function inkOn(fill: string): string {
  const l = luminance(fill);
  // Whichever of white and near-black contrasts more with the fill.
  return 1.05 / (l + 0.05) >= (l + 0.05) / 0.06 ? '#ffffff' : '#1b1a17';
}

/** A point on a ramp of colour steps: `t` in 0..1, linear between neighbouring steps. */
export function rampColor(ramp: string[], t: number): Rgb {
  const n = ramp.length - 1;
  const x = Math.min(1, Math.max(0, t)) * n;
  const i = Math.min(n - 1, Math.floor(x));
  const f = x - i;
  const a = parseColor(ramp[i] ?? '#000');
  const b = parseColor(ramp[i + 1] ?? '#000');
  return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f];
}

export function rampCss(ramp: string[], t: number): string {
  const [r, g, b] = rampColor(ramp, t);
  return `rgb(${Math.round(r)} ${Math.round(g)} ${Math.round(b)})`;
}

/** A 256-entry RGB lookup table of a ramp, for writing `ImageData` in a tight loop. */
export function rampTable(ramp: string[]): Uint8ClampedArray {
  const table = new Uint8ClampedArray(256 * 3);
  for (let i = 0; i < 256; i++) {
    const [r, g, b] = rampColor(ramp, i / 255);
    table[i * 3] = r;
    table[i * 3 + 1] = g;
    table[i * 3 + 2] = b;
  }
  return table;
}
