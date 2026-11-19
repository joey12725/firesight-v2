// Scrolling inside the results workspace. The headline strip and the tabs live inside the
// scrolling pane (pinned on roomy screens, scrolling away on small or short ones), so a
// target has to clear whatever is pinned at the top at that moment.

/** The pane that scrolls the results (the workspace's `<main>`). */
export function resultsPane(): HTMLElement | null {
  return document.querySelector<HTMLElement>('[data-results-scroll]');
}

/** Height of whatever is pinned to the top of the pane right now: the whole head, or only the tabs. */
export function pinnedHeight(pane: HTMLElement): number {
  const head = pane.querySelector<HTMLElement>('[data-results-head]');
  const tabs = pane.querySelector<HTMLElement>('[data-results-tabs]');
  if (head && getComputedStyle(head).display !== 'contents') return head.offsetHeight;
  return tabs?.offsetHeight ?? 0;
}

/** Brings an element just below the pinned head of the results pane, without scrolling the document behind the app. */
export function scrollResultsTo(target: Element | null, margin = 20) {
  const pane = resultsPane();
  if (!target || !pane) return;
  const top = pane.scrollTop + target.getBoundingClientRect().top - pane.getBoundingClientRect().top - pinnedHeight(pane) - margin;
  pane.scrollTo({ top: Math.max(0, top), behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' });
}
