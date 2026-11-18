// Theme-aware colour access for chart code. Canvas cannot resolve `var(--x)`, so charts
// read the resolved token values through this hook and redraw when the theme changes.
import { useSyncExternalStore } from 'react';

/** Every chart token in tokens.css, resolved to a CSS colour string. */
export interface ChartTheme {
  dark: boolean;
  fontSans: string;
  fontMono: string;
  surface: string;
  ink: { primary: string; secondary: string; tertiary: string };
  grid: string;
  axis: string;
  label: string;
  annotation: string;
  /** The plan as entered. `bands` runs outer (5-95) to inner (25-75). */
  baseline: { line: string; median: string; bands: [string, string, string]; samplePath: string };
  /** A scenario under comparison. */
  scenario: { line: string; bands: [string, string, string] };
  status: { working: string; retired: string; cutting: string; depleted: string; dead: string };
  /** Categorical series colours, in the order to assign them. */
  categorical: string[];
  /** Sequential ramp for heat maps, low to high. */
  sequential: string[];
  semantic: { good: string; warn: string; bad: string; accent: string; cool: string };
}

function read(): ChartTheme {
  const style = getComputedStyle(document.documentElement);
  const v = (name: string) => style.getPropertyValue(name).trim();
  return {
    dark: document.documentElement.dataset.theme === 'dark',
    fontSans: v('--ff-sans'),
    fontMono: v('--ff-mono'),
    surface: v('--surface-1'),
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
    },
    scenario: {
      line: v('--chart-scenario'),
      bands: [v('--chart-scenario-band-outer'), v('--chart-scenario-band-mid'), v('--chart-scenario-band-inner')],
    },
    status: {
      working: v('--chart-status-working'),
      retired: v('--chart-status-retired'),
      cutting: v('--chart-status-cutting'),
      depleted: v('--chart-status-depleted'),
      dead: v('--chart-status-dead'),
    },
    categorical: [1, 2, 3, 4, 5, 6, 7, 8].map((i) => v(`--chart-cat-${i}`)),
    sequential: [1, 2, 3, 4, 5].map((i) => v(`--chart-seq-${i}`)),
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
