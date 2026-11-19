// README screenshots at 2x, written to docs/images at the repository root:
//   BASE_URL=http://localhost:4173 node e2e/docs-shots.mjs && python3 scripts/optimise-images.py
// Every figure on screen is real engine output for the demo households (10,000 lives,
// seed 1). The one thing scripted is the assistant's reply in `editor-assistant.png`: the
// request to api.anthropic.com is answered by this script with a fixed tool call, so the
// change card it shows is the app's own, computed by the engine from the open plan. The
// scripted text states no figures of its own.
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5173';
const out = resolve(dirname(fileURLToPath(import.meta.url)), '../../docs/images');
mkdirSync(out, { recursive: true });
const only = process.argv[2]?.split(',');
const want = (name) => !only || only.includes(name);
const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];

async function open(slug, { theme = 'light', editor = false, assistant = false, width = 1440, height = 900, settings = {} } = {}) {
  const context = await browser.newContext({ viewport: { width, height }, colorScheme: theme, deviceScaleFactor: 2 });
  const page = await context.newPage();
  page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && !/status of 404/.test(m.text()) && problems.push(`${slug} console ${m.type()}: ${m.text().slice(0, 200)}`));
  page.on('pageerror', (e) => problems.push(`${slug} pageerror: ${e.message}`));
  await page.goto(base);
  await page.getByTestId(`demo-${slug}`).click();
  await page.evaluate(({ editor, assistant, settings }) => window.__firesight.settings.getState().set({ editorOpen: editor, assistantOpen: assistant, ...settings }), { editor, assistant, settings });
  await settled(page);
  return { context, page };
}
const settled = (page) => page.waitForFunction(() => { const r = window.__firesight.run.getState(); return r.status === 'done' && !r.stale && r.scenarios.every((s) => s.paired); }, null, { timeout: 240_000 });
const go = async (page, view) => {
  await page.getByRole('tab', { name: view, exact: true }).click();
  await page.waitForTimeout(900);
};
/** Grows the window so the results pane does not scroll, then captures the union of the given elements. */
async function capture(page, name, locators, { pad = 24, maxHeight = 20_000, ready } = {}) {
  const { width } = page.viewportSize();
  const need = await page.evaluate(() => { const p = document.querySelector('[data-results-scroll]'); return p ? p.scrollHeight - p.clientHeight : 0; });
  await page.setViewportSize({ width, height: Math.min(9000, 900 + need + 40) });
  await page.waitForTimeout(900);
  // Anything that depends on the pointer (a hover) is set up after the window has its final size.
  if (ready) await ready();
  const boxes = [];
  for (const l of locators) boxes.push(await l.boundingBox());
  const x0 = Math.max(0, Math.min(...boxes.map((b) => b.x)) - pad);
  const y0 = Math.max(0, Math.min(...boxes.map((b) => b.y)) - pad);
  const x1 = Math.min(width, Math.max(...boxes.map((b) => b.x + b.width)) + pad);
  const y1 = Math.min(y0 + maxHeight, Math.max(...boxes.map((b) => b.y + b.height)) + pad);
  await page.screenshot({ path: `${out}/${name}.png`, clip: { x: x0, y: y0, width: x1 - x0, height: y1 - y0 } });
  console.log(`  ${name}.png  ${Math.round(x1 - x0)}x${Math.round(y1 - y0)} css px`);
}

try {
  if (want('hero-overview')) {
    // Tall enough to take in the fan below the opening sentence.
    const { context, page } = await open('dual-income-family', { editor: true, height: 1500 });
    await page.waitForTimeout(800);
    await page.screenshot({ path: `${out}/hero-overview.png` });
    console.log('  hero-overview.png');
    await context.close();
  }

  if (want('net-worth-fan') || want('what-matters') || want('cash-flow') || want('life-stories')) {
    const { context, page } = await open('dual-income-family', { width: 1280 });
    if (want('net-worth-fan')) {
      const card = page.locator('[data-chart-card="net-worth-by-age"]');
      await card.scrollIntoViewIfNeeded();
      // Pick out the named unlucky life, as hovering its link in the caption does.
      const hover = async () => {
        await card.getByRole('button', { name: 'an unlucky life' }).hover();
        await page.waitForTimeout(400);
      };
      await capture(page, 'net-worth-fan', [card], { ready: hover });
      await page.mouse.move(5, 5);
      await page.setViewportSize({ width: 1280, height: 900 });
    }
    if (want('cash-flow')) {
      await go(page, 'Cash flow');
      await page.getByTestId('cash-flow-lead').waitFor();
      await capture(page, 'cash-flow', [page.locator('[data-chart-card="money-in-and-out-by-age"]'), page.locator('[data-chart-card^="money-flow-age"]')]);
      await page.setViewportSize({ width: 1280, height: 900 });
    }
    if (want('life-stories')) {
      await go(page, 'Life stories');
      await page.getByTestId('life-headline').waitFor({ timeout: 60_000 });
      await page.waitForTimeout(800);
      await capture(page, 'life-stories', [page.getByTestId('life-headline'), page.getByTestId('life-narrative'), page.locator('[data-chart-card^="life-"]').first()]);
      await page.setViewportSize({ width: 1280, height: 900 });
    }
    if (want('what-matters')) {
      await go(page, 'What matters');
      await page.getByTestId('lead-sentence').waitFor({ timeout: 120_000 });
      const map = page.locator('[data-chart-card="trade-off-map"]');
      await map.scrollIntoViewIfNeeded();
      await page.waitForFunction(() => { const t = document.querySelector('[data-chart-card="trade-off-map"]')?.textContent ?? ''; const all = document.querySelector('main')?.textContent ?? ''; return /lives per cell/.test(t) && !/coarse pass/.test(t) && !/so far/.test(t) && !/Sharpening|Early picture|Updating/.test(all); }, null, { timeout: 420_000 });
      await page.waitForTimeout(600);
      await capture(page, 'what-matters', [page.locator('[data-chart-card^="what-moves"]'), map]);
    }
    await context.close();
  }

  if (want('scenarios')) {
    const { context, page } = await open('dual-income-family', { width: 1280 });
    await go(page, 'Scenarios');
    for (const key of ['retire_later', 'delay_social_security']) {
      const button = page.getByTestId(`quick-${key}`);
      if (await button.count()) await button.click();
    }
    await page.waitForFunction(() => window.__firesight.plan.getState().scenarios.length >= 1);
    await settled(page);
    await page.waitForTimeout(800);
    const detail = page.getByTestId('scenario-detail');
    await capture(page, 'scenarios', [detail], { maxHeight: 1500 });
    await context.close();
  }

  if (want('editor-assistant')) {
    const { context, page } = await open('dual-income-family', { editor: true, assistant: true, settings: { apiKey: 'sk-ant-scripted-for-a-screenshot' } });
    // The scripted model: one reply that drafts a change, as server-sent events in the API's own format.
    const input = {
      summary: 'Raise Daniel’s 401(k) contribution and pay the minivan loan down faster',
      ops: [
        { op: 'set', path: '/incomes/@daniel_job/kind/plan/employee_fraction', value: 0.12 },
        { op: 'set', path: '/debt_policy/strategy', value: 'avalanche' },
        { op: 'set', path: '/debt_policy/extra_monthly', value: 300 },
      ],
      assumptions: [
        { field: 'Daniel’s 401(k) contribution', value: '12% of salary', basis: 'stated', confidence: 'high' },
        { field: 'Extra debt payment', value: '$300 a month', basis: 'stated', confidence: 'high' },
        { field: 'Which debt gets the extra', value: 'highest rate first', basis: 'inferred', confidence: 'medium' },
      ],
    };
    const text = 'I have drafted both changes as one card: Daniel’s contribution rate, and the extra payment on the minivan loan. Nothing in the plan changes until you accept it; after that the simulation re-runs on the same lives, so the before and after can be compared life by life.';
    const usage = { input_tokens: 100, output_tokens: 60, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 };
    const events = [
      { type: 'message_start', message: { id: 'msg_scripted', type: 'message', role: 'assistant', model: 'claude-opus-5-5', content: [], stop_reason: null, stop_sequence: null, usage } },
      { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } },
      { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } },
      { type: 'content_block_stop', index: 0 },
      { type: 'content_block_start', index: 1, content_block: { type: 'tool_use', id: 'toolu_scripted', name: 'propose_changes', input: {} } },
      { type: 'content_block_delta', index: 1, delta: { type: 'input_json_delta', partial_json: JSON.stringify(input) } },
      { type: 'content_block_stop', index: 1 },
      { type: 'message_delta', delta: { stop_reason: 'tool_use', stop_sequence: null }, usage },
      { type: 'message_stop' },
    ];
    const cors = { 'access-control-allow-origin': '*', 'access-control-allow-headers': '*', 'access-control-allow-methods': 'POST, OPTIONS' };
    let calls = 0;
    await page.route('https://api.anthropic.com/**', (route) => {
      if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204, headers: cors });
      calls += 1;
      return route.fulfill({ status: 200, headers: { ...cors, 'content-type': 'text/event-stream' }, body: events.map((e) => `event: ${e.type}\ndata: ${JSON.stringify(e)}\n\n`).join('') });
    });
    const panel = page.getByRole('complementary', { name: 'Assistant' });
    const box = panel.getByRole('textbox').first();
    await box.waitFor({ timeout: 30_000 });
    await box.fill('Daniel is raising his 401(k) from 8% to 12%, and we want to put an extra $300 a month toward the minivan loan.');
    await box.press('Enter');
    await panel.getByRole('button', { name: /Accept/ }).first().waitFor({ timeout: 60_000 });
    await page.waitForTimeout(700);
    if (calls !== 1) problems.push(`expected one scripted model call, saw ${calls}`);
    await page.screenshot({ path: `${out}/editor-assistant.png` });
    console.log('  editor-assistant.png');
    await context.close();
  }

  if (want('outcomes-dark')) {
    const { context, page } = await open('early-career-saver', { theme: 'dark', width: 1280 });
    await go(page, 'Outcomes');
    await page.getByTestId('range-sentence').waitFor();
    await page.waitForTimeout(800);
    await capture(page, 'outcomes-dark', [page.getByTestId('range-sentence'), page.locator('[data-chart-card="retirement-ages"]'), page.locator('[data-chart-card="net-worth-at-end"]'), page.locator('[data-chart-card="deepest-spending-cut"]'), page.locator('[data-chart-card="lifetime-tax"]')], { pad: 32 });
    await context.close();
  }
} catch (error) {
  problems.push(error instanceof Error ? (error.stack ?? error.message) : String(error));
}
await browser.close();
if (problems.length) {
  console.log('Problems:\n' + [...new Set(problems)].join('\n'));
  process.exitCode = 1;
}
