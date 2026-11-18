/**
 * Sizes a canvas for the device pixel ratio and returns a context whose units are CSS
 * pixels. Call it at the start of every draw; it also clears the canvas.
 */
export function setupCanvas(canvas: HTMLCanvasElement, width: number, height: number): CanvasRenderingContext2D | null {
  const dpr = Math.min(window.devicePixelRatio || 1, 3);
  const w = Math.max(1, Math.round(width * dpr));
  const h = Math.max(1, Math.round(height * dpr));
  if (canvas.width !== w) canvas.width = w;
  if (canvas.height !== h) canvas.height = h;
  canvas.style.width = `${width}px`;
  canvas.style.height = `${height}px`;
  const ctx = canvas.getContext('2d');
  if (!ctx) return null;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);
  return ctx;
}

/** Snaps a coordinate so a 1px line is crisp. */
export function crisp(value: number): number {
  return Math.round(value) + 0.5;
}

/** True when the user asked for reduced motion; charts should then skip transitions. */
export function prefersReducedMotion(): boolean {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}
