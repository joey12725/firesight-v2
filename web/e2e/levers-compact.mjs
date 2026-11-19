// Screenshots of the assistant's compact tornado and heat map at chat width, from real sweeps:
//   BASE_URL=http://localhost:5183 node e2e/levers-compact.mjs
// Writes e2e/shots/levers-compact-<theme>.png.
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5183';
const shots = resolve(dirname(fileURLToPath(import.meta.url)), 'shots');
const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];
for (const theme of ['light', 'dark']) {
  const page = await browser.newPage({ viewport: { width: 1100, height: 520 }, colorScheme: theme });
  page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && !/status of 404/.test(m.text()) && problems.push(`${theme} console ${m.type()}: ${m.text()}`));
  page.on('pageerror', (e) => problems.push(`${theme} pageerror: ${e.message}`));
  await page.goto(base);
  await page.getByTestId('demo-dual-income-family').click();
  await page.waitForFunction(() => window.__firesight.run.getState().status === 'done', null, { timeout: 180_000 });
  await page.evaluate(async () => (await import('/e2e/levers-compact.tsx')).mount());
  await page.locator('#levers-compact [data-chart-frame]').nth(2).waitFor({ timeout: 240_000 });
  await page.waitForTimeout(600);
  await page.screenshot({ path: `${shots}/levers-compact-${theme}.png` });
  await page.close();
}
await browser.close();
if (problems.length) {
  console.log('Problems:\n' + problems.join('\n'));
  process.exitCode = 1;
}
