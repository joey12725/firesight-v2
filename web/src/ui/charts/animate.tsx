// Animation between successive results. A run refines several times a second; charts
// glide from one summary to the next instead of jumping. Everything here stands still
// when the user prefers reduced motion.
import { useLayoutEffect, useRef, useState, type HTMLAttributes } from 'react';
import { prefersReducedMotion } from './canvas';

export const easeOut = (t: number) => 1 - (1 - t) ** 3;

/**
 * Interpolates two values of the same shape: numbers blend, arrays and plain objects
 * blend element by element, anything else (strings, nulls, arrays whose length
 * changed) takes the target value at once.
 */
export function interpolate<T>(a: T, b: T, t: number): T {
  if (t >= 1 || a === b) return b;
  if (typeof a === 'number' && typeof b === 'number') return (a + (b - a) * t) as T;
  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) return b;
    const out = new Array<unknown>(b.length);
    for (let i = 0; i < b.length; i++) out[i] = interpolate(a[i], b[i], t);
    return out as T;
  }
  if (a && b && typeof a === 'object' && typeof b === 'object' && !ArrayBuffer.isView(a)) {
    const out: Record<string, unknown> = {};
    for (const key of Object.keys(b)) out[key] = interpolate((a as Record<string, unknown>)[key], (b as Record<string, unknown>)[key], t);
    return out as T;
  }
  return b;
}

export interface AnimateOptions {
  /** Milliseconds. Default 320. */
  duration?: number;
  /** When this changes the value snaps (a different plan, a different series). */
  resetKey?: unknown;
  /** Set false to snap (for example while the user drags a control). */
  enabled?: boolean;
}

export interface AnimateWithOptions<T> extends AnimateOptions {
  /** Blends two values; defaults to the structural `interpolate`. Must be a stable function. */
  blend?: (from: T, to: T, t: number) => T;
}

/**
 * Follows `target` with a short eased transition. Give it only what the chart draws
 * (a model derived with useMemo), not a whole Summary. A new target mid-flight starts
 * from wherever the value currently is, so progressive refinement never stutters.
 */
export function useAnimated<T>(target: T, { duration = 320, resetKey, enabled = true, blend = interpolate }: AnimateWithOptions<T> = {}): T {
  const [state, setState] = useState({ value: target, key: resetKey });
  // A different plan or series: adopt the new value during render, with no transition.
  if (state.key !== resetKey) setState({ value: target, key: resetKey });
  const current = useRef(target);
  const lastKey = useRef(resetKey);

  useLayoutEffect(() => {
    if (lastKey.current !== resetKey || !enabled) {
      lastKey.current = resetKey;
      current.current = target;
      return;
    }
    const from = current.current;
    if (from === target) return;
    const instant = prefersReducedMotion();
    const start = performance.now();
    let raf = requestAnimationFrame(function step(now: number) {
      const t = instant ? 1 : Math.min(1, (now - start) / duration);
      const next = blend(from, target, easeOut(t));
      current.current = next;
      setState({ value: next, key: resetKey });
      if (t < 1) raf = requestAnimationFrame(step);
    });
    return () => cancelAnimationFrame(raf);
  }, [target, duration, resetKey, enabled, blend]);

  return state.key !== resetKey || !enabled ? target : state.value;
}

export interface AnimatedNumberProps extends Omit<HTMLAttributes<HTMLSpanElement>, 'children'> {
  /** Null shows `placeholder`. */
  value: number | null | undefined;
  format: (value: number) => string;
  duration?: number;
  placeholder?: string;
}

/**
 * A number that counts smoothly to its new value (headline figures). It writes the text
 * directly, so a tween costs no React renders; screen readers get the final value only.
 */
export function AnimatedNumber({ value, format, duration = 400, placeholder = '–', ...rest }: AnimatedNumberProps) {
  const ref = useRef<HTMLSpanElement>(null);
  const shown = useRef<number | null>(null);
  const final = value === null || value === undefined || !Number.isFinite(value) ? null : value;

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const from = shown.current;
    if (final === null || from === null || prefersReducedMotion() || document.hidden) {
      shown.current = final;
      el.textContent = final === null ? placeholder : format(final);
      return;
    }
    const start = performance.now();
    let raf = requestAnimationFrame(function step(now: number) {
      const t = Math.min(1, (now - start) / duration);
      const v = from + (final - from) * easeOut(t);
      shown.current = v;
      el.textContent = format(t >= 1 ? final : v);
      if (t < 1) raf = requestAnimationFrame(step);
    });
    return () => cancelAnimationFrame(raf);
    // `format` is expected to be stable in meaning; a new closure alone should not restart the tween.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [final, duration, placeholder]);

  return (
    <span {...rest}>
      {/* The visible text is written by the effect above, so React leaves it alone between renders. */}
      <span ref={ref} aria-hidden />
      <span className="sr-only">{final === null ? 'not available' : format(final)}</span>
    </span>
  );
}
