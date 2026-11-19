// Performance probe against a running dev or preview server:
//   BASE_URL=http://localhost:5173 node e2e/perf.mjs [slug] [paths,paths,...]
// For each path count: wall time, lives per second, time to first picture, the cost of
// the last merge + summarise in the coordinator, bytes that crossed threads, the
// longest main-thread task and frame gap during the run, and memory.
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5173';
const slug = process.argv[2] ?? 'dual-income-family';
const counts = (process.argv[3] ?? '10000,100000,250000').split(',').map(Number);

const browser = await chromium.launch({ channel: 'chrome', args: ['--enable-precise-memory-info'] }).catch(() => chromium.launch());
const page = await (await browser.newContext({ viewport: { width: 1440, height: 900 } })).newPage();
page.on('pageerror', (e) => console.log('pageerror', e.message));
page.on('console', (m) => m.type() === 'error' && !/404/.test(m.text()) && console.log('console error', m.text()));
await page.goto(base + (process.env.WORKERS ? `?workers=${process.env.WORKERS}` : ''));
await page.getByTestId(`demo-${slug}`).click();
await page.waitForFunction(() => window.__firesight?.run.getState().status === 'done', null, { timeout: 120_000 });
const cores = await page.evaluate(() => navigator.hardwareConcurrency);
console.log(`${slug}: ${cores} logical cores, ${await page.evaluate(() => window.__firesight.pool().size)} engine workers + 1 coordinator`);

for (const paths of counts) {
  const result = await page.evaluate(async (paths) => {
    const fs = window.__firesight;
    const stats = fs.stats;
    const before = { ...stats };
    const poolBefore = { ...fs.pool().stats };
    let longest = 0;
    const observer = new PerformanceObserver((list) => {
      for (const e of list.getEntries()) longest = Math.max(longest, e.duration);
    });
    observer.observe({ entryTypes: ['longtask'] });
    let worstFrame = 0;
    let frames = 0;
    let last = performance.now();
    let watching = true;
    const tick = (t) => {
      worstFrame = Math.max(worstFrame, t - last);
      last = t;
      frames += 1;
      if (watching) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    let peakHeap = 0;
    let summaries = 0;
    const unsubscribe = fs.run.subscribe((s, p) => {
      if (s.summary !== p.summary) summaries += 1;
      peakHeap = Math.max(peakHeap, performance.memory?.usedJSHeapSize ?? 0);
    });
    const t0 = performance.now();
    fs.plan.getState().setRun({ paths, seed: fs.plan.getState().run.seed + 1 });
    await new Promise((resolve) => {
      const stop = fs.run.subscribe((s) => {
        if (s.status === 'done' && s.total === paths) {
          stop();
          resolve();
        }
      });
    });
    const wall = performance.now() - t0;
    await new Promise((r) => setTimeout(r, 300));
    watching = false;
    observer.disconnect();
    unsubscribe();
    const run = fs.run.getState();
    return {
      paths,
      wallMs: Math.round(wall),
      livesPerSecond: Math.round((paths / wall) * 1000),
      firstPictureMs: Math.round(run.firstPictureMs),
      summaries,
      lastSummarizeMs: +stats.lastSummarizeMs.toFixed(1),
      lastParseMs: +stats.lastParseMs.toFixed(1),
      summaryKB: Math.round((stats.summaryBytes - before.summaryBytes) / Math.max(1, stats.summaries - before.summaries) / 1024),
      workerUtilisation: +((fs.pool().stats.workerMs - poolBefore.workerMs) / (wall * fs.pool().size)).toFixed(3),
      workerMsPerLife: +((fs.pool().stats.workerMs - poolBefore.workerMs) / paths).toFixed(3),
      chunks: stats.chunks - before.chunks,
      chunkMB: +((stats.chunkBytes - before.chunkBytes) / 1048576).toFixed(1),
      longestTaskMs: Math.round(longest),
      worstFrameMs: Math.round(worstFrame),
      fps: Math.round((frames / wall) * 1000),
      mainHeapPeakMB: Math.round(peakHeap / 1048576),
      coordinatorWasmMB: Math.round(stats.coordinatorHeapBytes / 1048576),
    };
  }, paths);
  console.log(JSON.stringify(result));
}

// Hover on the fan with the largest run on screen: frame pacing while the crosshair moves.
{
  const fan = page.locator('[data-chart-card^="net-worth"] [data-chart-cursor]').first();
  await fan.scrollIntoViewIfNeeded();
  const box = await fan.boundingBox();
  await page.evaluate(() => {
    const w = window;
    w.__frames = [];
    w.__long = 0;
    new PerformanceObserver((list) => list.getEntries().forEach((e) => (w.__long = Math.max(w.__long, e.duration)))).observe({ entryTypes: ['longtask'] });
    let last = performance.now();
    const tick = (t) => {
      w.__frames.push(t - last);
      last = t;
      if (w.__frames.length < 100000) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  const t0 = Date.now();
  let moves = 0;
  while (Date.now() - t0 < 3000) {
    const f = ((Date.now() - t0) % 1500) / 1500;
    await page.mouse.move(box.x + 4 + f * (box.width - 8), box.y + box.height * (0.3 + 0.4 * Math.sin(f * 12)));
    moves += 1;
  }
  const hover = await page.evaluate(() => {
    const f = window.__frames.slice(5).sort((a, b) => a - b);
    return { frames: f.length, medianMs: +f[Math.floor(f.length / 2)].toFixed(1), p95Ms: +f[Math.floor(f.length * 0.95)].toFixed(1), worstMs: +f[f.length - 1].toFixed(1), fps: Math.round(1000 / (f.reduce((a, b) => a + b, 0) / f.length)), longestTaskMs: Math.round(window.__long) };
  });
  console.log(`hover on the fan at ${counts[counts.length - 1]} paths (${moves} pointer moves in 3 s)`, JSON.stringify(hover));
}

// Cancellation under load: start a large run, then edit while it is in flight.
const cancel = await page.evaluate(async () => {
  const fs = window.__firesight;
  fs.plan.getState().setRun({ paths: 250_000 });
  await new Promise((r) => setTimeout(r, 1500));
  const midway = fs.run.getState().done;
  const t0 = performance.now();
  fs.plan.getState().setRun({ paths: 2_000, seed: 99 });
  const first = await new Promise((resolve) => {
    const stop = fs.run.subscribe((s, p) => {
      if (s.total === 2_000 && s.summary !== p.summary && !s.stale) {
        stop();
        resolve(performance.now() - t0);
      }
    });
  });
  await new Promise((resolve) => {
    const stop = fs.run.subscribe((s) => {
      if (s.status === 'done') {
        stop();
        resolve();
      }
    });
  });
  return { cancelledAt: midway, firstPictureAfterEditMs: Math.round(first), newRunDoneMs: Math.round(performance.now() - t0), activeJobs: fs.pool().activeJobs };
});
console.log('cancel under load', JSON.stringify(cancel));
await browser.close();
