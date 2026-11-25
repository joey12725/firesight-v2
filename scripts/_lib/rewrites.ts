// Applies the rewrites in vercel.json to a request path, so the local dev server routes
// exactly as the deployment does. Only the path syntax this project uses is supported:
// literal segments, `:name` (one segment) and a trailing `:name*` (the rest, possibly
// empty). Sources written as regular expressions (the single-page-app fallback) are
// skipped: the dev API server serves no pages.

export interface Rewrite {
  source: string;
  destination: string;
}

/** The rewritten path and query, or null when no rule matches. */
export function applyRewrites(rewrites: readonly Rewrite[], pathname: string, search: string): string | null {
  for (const rule of rewrites) {
    if (rule.source.includes('(')) continue;
    const params = match(rule.source, pathname);
    if (!params) continue;
    let destination = rule.destination;
    // Longest names first, so `:id` does not eat the front of `:ident`.
    for (const name of Object.keys(params).sort((a, b) => b.length - a.length)) {
      destination = destination.replaceAll(new RegExp(`:${name}\\*?`, 'g'), params[name] ?? '');
    }
    // The request's own query is kept, after the destination's.
    const extra = search.replace(/^\?/, '');
    if (!extra) return destination;
    return `${destination}${destination.includes('?') ? '&' : '?'}${extra}`;
  }
  return null;
}

function match(source: string, pathname: string): Record<string, string> | null {
  const want = source.split('/').slice(1);
  const have = pathname.split('/').slice(1);
  const params: Record<string, string> = {};
  for (let i = 0; i < want.length; i++) {
    const segment = want[i] ?? '';
    if (segment.startsWith(':') && segment.endsWith('*')) {
      if (i !== want.length - 1) return null;
      params[segment.slice(1, -1)] = have.slice(i).join('/');
      return params;
    }
    const actual = have[i];
    if (actual === undefined) return null;
    if (segment.startsWith(':')) {
      if (actual === '') return null;
      params[segment.slice(1)] = actual;
    } else if (segment !== actual) {
      return null;
    }
  }
  return have.length === want.length ? params : null;
}
