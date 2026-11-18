// Main-thread cost of one crosshair move on the fan (pointer event -> React render ->
// DOM updated), measured by dispatching moves back to back at the path count given.
// Independent of the display's refresh rate, unlike counting frames in a headless browser.
//   BASE_URL=http://localhost:4173 node e2e/hover-cost.mjs [paths]
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5173';
const paths = Number(process.argv[2] ?? 100000);
const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const page = await (await browser.newContext({ viewport: { width: 1440, height: 900 } })).newPage();
await page.goto(base);
await page.getByTestId('demo-dual-income-family').click();
await page.waitForFunction((n) => {
  const fs = window.__firesight;
  if (!fs) return false;
  if (fs.plan.getState().run.paths !== n) fs.plan.getState().setRun({ paths: n });
  const run = fs.run.getState();
  return run.status === 'done' && run.total === n;
}, paths, { timeout: 300_000 });
const result = await page.evaluate(async () => {
  const el = document.querySelector('[data-chart-card^="net-worth"] [data-chart-cursor]');
  el.scrollIntoView();
  const r = el.getBoundingClientRect();
  const tick = () => new Promise((resolve) => { const c = new MessageChannel(); c.port1.onmessage = resolve; c.port2.postMessage(0); });
  const costs = [];
  for (let i = 0; i < 400; i++) {
    const f = (i % 200) / 200;
    const t0 = performance.now();
    el.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: r.left + 4 + f * (r.width - 8), clientY: r.top + r.height * (0.3 + 0.4 * Math.abs(Math.sin(i / 9))), pointerType: 'mouse' }));
    await tick();
    await tick();
    costs.push(performance.now() - t0);
  }
  costs.sort((a, b) => a - b);
  return { moves: costs.length, medianMs: +costs[200].toFixed(2), p95Ms: +costs[380].toFixed(2), worstMs: +costs[399].toFixed(2), tooltipShown: !!document.querySelector('[data-chart-card^="net-worth"] [role="presentation"]') };
});
console.log(`crosshair move on the fan at ${paths} lives:`, JSON.stringify(result), `=> ${Math.round(1000 / result.p95Ms)} updates a second possible at the 95th percentile`);
await browser.close();
