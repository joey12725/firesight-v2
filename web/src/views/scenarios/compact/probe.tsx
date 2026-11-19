// Loaded by e2e/scenarios.mjs (COMPACT=1) through the dev server: mounts the assistant's
// `compare_delta` chart at chat width from the live run, so it can be looked at without
// a model connection. Nothing in the app imports it, so it is not in the bundle.
import { createRoot } from 'react-dom/client';
import type { Diagnostics } from '@/state/diagnostics';
import CompareDelta from './CompareDelta';

export function mount(paired: boolean) {
  const fs = (window as unknown as { __firesight: Diagnostics }).__firesight;
  const run = fs.run.getState();
  const scenario = run.scenarios[run.scenarios.length - 1];
  const host = document.createElement('div');
  host.id = 'compact-probe';
  host.style.cssText = 'position:fixed;left:16px;top:16px;width:320px;padding:12px;z-index:9999;background:var(--surface-0);border:1px solid var(--line);border-radius:8px';
  document.body.appendChild(host);
  if (!scenario?.summary) return;
  createRoot(host).render(
    <CompareDelta kind="compare_delta" compact caption="" summary={run.summary} profile={fs.plan.getState().profile} comparison={{ name: scenario.name, summary: scenario.summary, paired: paired ? scenario.paired : null }} />,
  );
}
