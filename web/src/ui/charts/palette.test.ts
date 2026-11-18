// The chart palette, checked by computation rather than by eye, in both themes.
// For every palette whose marks sit side by side: lightness inside the band where a
// colour still reads as a colour, enough chroma to carry identity, neighbours that stay
// apart under protanopia and deuteranopia (Machado, Oliveira & Fernandes 2009, severity
// 1.0) and under normal vision, and contrast against the page. Thresholds follow the
// data-visualisation method the kit is built on; distances are Euclidean in OKLab x 100.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

type Rgb = [number, number, number];

const css = readFileSync(new URL('../../styles/tokens.css', import.meta.url), 'utf8');
const split = css.indexOf(":root[data-theme='dark']");
const blocks = { light: css.slice(0, split), dark: css.slice(split) };

function token(mode: 'light' | 'dark', name: string): string {
  const m = new RegExp(`--${name}:\\s*(#[0-9a-fA-F]{6})\\b`).exec(blocks[mode]);
  if (!m?.[1]) throw new Error(`token --${name} is missing or not a hex colour in the ${mode} theme`);
  return m[1];
}

const lin = (hex: string): Rgb =>
  [1, 3, 5].map((i) => {
    const s = parseInt(hex.slice(i, i + 2), 16) / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  }) as Rgb;

function oklab([r, g, b]: Rgb): Rgb {
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s, 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s];
}

const MACHADO: Record<'protan' | 'deutan', number[][]> = {
  protan: [
    [0.152286, 1.052583, -0.204868],
    [0.114503, 0.786281, 0.099216],
    [-0.003882, -0.048116, 1.051998],
  ],
  deutan: [
    [0.367322, 0.860646, -0.227968],
    [0.280085, 0.672501, 0.047413],
    [-0.01182, 0.04294, 0.968881],
  ],
};

function simulate(hex: string, kind: 'protan' | 'deutan'): Rgb {
  const [r, g, b] = lin(hex);
  return (MACHADO[kind] as number[][]).map((row) => Math.max(0, Math.min(1, (row[0] as number) * r + (row[1] as number) * g + (row[2] as number) * b))) as Rgb;
}

function deltaE(a: string, b: string, kind?: 'protan' | 'deutan'): number {
  const x = oklab(kind ? simulate(a, kind) : lin(a));
  const y = oklab(kind ? simulate(b, kind) : lin(b));
  return 100 * Math.hypot(x[0] - y[0], x[1] - y[1], x[2] - y[2]);
}

const lightness = (hex: string) => oklab(lin(hex))[0];
const chroma = (hex: string) => Math.hypot(oklab(lin(hex))[1], oklab(lin(hex))[2]);
const luminance = (hex: string) => {
  const [r, g, b] = lin(hex);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const contrast = (a: string, b: string) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
};

const BAND = { light: [0.43, 0.77], dark: [0.48, 0.67] } as const;
const MODES = ['light', 'dark'] as const;

/** The categorical checks on a run of colours whose neighbours touch (stacks, bars, lines). */
function checkAdjacent(mode: 'light' | 'dark', names: string[], { relief = [] as string[] } = {}) {
  const surface = token(mode, 'surface-0');
  const colours = names.map((n) => token(mode, n));
  colours.forEach((c, i) => {
    const name = names[i] as string;
    expect(lightness(c), `${name} lightness (${mode})`).toBeGreaterThanOrEqual(BAND[mode][0]);
    expect(lightness(c), `${name} lightness (${mode})`).toBeLessThanOrEqual(BAND[mode][1]);
    expect(chroma(c), `${name} chroma (${mode})`).toBeGreaterThanOrEqual(0.1);
    // Below 3:1 is allowed only for colours that always carry a direct label or sit in a legend with a table view.
    if (!relief.includes(name)) expect(contrast(c, surface), `${name} contrast on the page (${mode})`).toBeGreaterThanOrEqual(3);
    else expect(contrast(c, surface), `${name} contrast on the page (${mode})`).toBeGreaterThanOrEqual(2);
  });
  for (let i = 0; i + 1 < colours.length; i++) {
    const [a, b] = [colours[i] as string, colours[i + 1] as string];
    const pair = `${names[i]} / ${names[i + 1]} (${mode})`;
    expect(Math.min(deltaE(a, b, 'protan'), deltaE(a, b, 'deutan')), `${pair} under colour-vision deficiency`).toBeGreaterThanOrEqual(8);
    expect(deltaE(a, b), `${pair} under normal vision`).toBeGreaterThanOrEqual(15);
  }
}

describe('chart palette', () => {
  for (const mode of MODES) {
    describe(mode, () => {
      it('life-stage colours are distinct from their neighbours in stacking order', () => {
        // Bottom to top, as STATUS_STACK stacks them. Amber is light on a light page by nature; it is always labelled.
        checkAdjacent(mode, ['chart-status-depleted', 'chart-status-cutting', 'chart-status-on-plan', 'chart-status-surplus', 'chart-status-working'], { relief: ['chart-status-cutting'] });
        // "Died" is deliberately neutral (absence, not a series) but must still part from its neighbour and the page.
        const dead = token(mode, 'chart-status-dead');
        const working = token(mode, 'chart-status-working');
        expect(deltaE(dead, working)).toBeGreaterThanOrEqual(15);
        expect(Math.min(deltaE(dead, working, 'protan'), deltaE(dead, working, 'deutan'))).toBeGreaterThanOrEqual(8);
        expect(deltaE(dead, token(mode, 'chart-status-surplus'))).toBeGreaterThanOrEqual(15);
        expect(chroma(dead)).toBeLessThan(0.03);
        expect(contrast(dead, token(mode, 'surface-0'))).toBeGreaterThanOrEqual(1.25);
      });

      it('categorical colours are distinct in assignment order', () => {
        checkAdjacent(mode, [1, 2, 3, 4, 5, 6, 7].map((i) => `chart-cat-${i}`), { relief: ['chart-cat-4'] });
        // The eighth slot is "Other": neutral on purpose.
        expect(chroma(token(mode, 'chart-cat-8'))).toBeLessThan(0.03);
      });

      it('baseline and scenario can be told apart wherever they meet', () => {
        const a = token(mode, 'chart-baseline');
        const b = token(mode, 'chart-scenario');
        expect(Math.min(deltaE(a, b, 'protan'), deltaE(a, b, 'deutan'))).toBeGreaterThanOrEqual(12);
        expect(deltaE(a, b)).toBeGreaterThanOrEqual(20);
        for (const c of [a, b, token(mode, 'chart-median')]) expect(contrast(c, token(mode, 'surface-0'))).toBeGreaterThanOrEqual(3);
      });

      it('sequential ramps are one hue and run monotonically away from the page', () => {
        for (const prefix of ['chart-seq', 'chart-seq-cool']) {
          const ramp = [1, 2, 3, 4, 5, 6, 7, 8].map((i) => token(mode, `${prefix}-${i}`));
          const surface = token(mode, 'surface-0');
          const ls = ramp.map(lightness);
          for (let i = 1; i < ls.length; i++) {
            const step = mode === 'light' ? (ls[i - 1] as number) - (ls[i] as number) : (ls[i] as number) - (ls[i - 1] as number);
            expect(step, `${prefix} step ${i} to ${i + 1} (${mode})`).toBeGreaterThanOrEqual(0.045);
          }
          const contrasts = ramp.map((c) => contrast(c, surface));
          for (let i = 1; i < contrasts.length; i++) expect(contrasts[i] as number).toBeGreaterThan(contrasts[i - 1] as number);
          // Discrete ordered marks start at step 4, which must stand off the page.
          expect(contrasts[3] as number, `${prefix}-4 on the page (${mode})`).toBeGreaterThanOrEqual(2);
          const hues = ramp.slice(2).map((c) => {
            const [, x, y] = oklab(lin(c));
            return (Math.atan2(y, x) * 180) / Math.PI;
          });
          expect(Math.max(...hues) - Math.min(...hues)).toBeLessThan(12);
        }
      });

      it('scaffolding stays recessive but legible', () => {
        const surface = token(mode, 'surface-0');
        expect(contrast(token(mode, 'chart-grid'), surface)).toBeLessThan(1.35);
        expect(contrast(token(mode, 'chart-label'), surface)).toBeGreaterThanOrEqual(4.2);
        expect(contrast(token(mode, 'ink-2'), surface)).toBeGreaterThanOrEqual(4.5);
      });
    });
  }
});
