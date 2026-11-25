// Synced plans for the website (Vercel Function, Node.js runtime, Web handler).
//
//   GET    /api/plans             { plans: [meta] }             newest first, no bodies
//   GET    /api/plans?export=1    { exported_at, plans: [meta + plan] }
//   POST   /api/plans             { plan, name? }               → 201 meta
//   GET    /api/plans/:id         meta + { plan }
//   PUT    /api/plans/:id         { plan, name? }, If-Match: <rev> → 200 meta | 409 | 428
//   DELETE /api/plans/:id         → 204
//
//   meta = { id, name, rev, created_at, updated_at, updated_by, bytes }
//
// Any of them answers 401 `account_changed` when the request names, in an
// `x-firesight-user` header, a user other than the one the session cookie belongs to.
//
// Only the session cookie authenticates here. An API key or an OAuth token in the
// Authorization header is ignored: those belong to the MCP endpoint. Writes must come
// from this site's own pages (exact Origin match) with a JSON body. Every query is
// scoped to the signed-in user, and another user's plan answers 404, the same as a plan
// that does not exist. Nothing is cached and nothing is logged.

import { sessionUser } from './_lib/credentials.js';
import { DecryptError } from './_lib/crypto.js';
import type { Env } from './_lib/env.js';
import { fromThisSite, json, misconfigured, notConfigured, notFromAnotherSite, problem, readJson } from './_lib/http.js';
import { MAX_PLAN_BYTES, PlanError } from './_lib/plans.js';
import type { PlanMeta } from './_lib/plans.js';
import { consumeRate, userRateKey } from './_lib/ratelimit.js';
import { runtimeFor } from './_lib/runtime.js';
import type { RuntimeState } from './_lib/runtime.js';

declare const process: { env: Env };

export interface Deps {
  state?: RuntimeState;
  now?: () => number;
  report?: (error: unknown) => void;
}

/** The plan limit plus room for the JSON envelope and a name. */
export const MAX_BODY_BYTES = MAX_PLAN_BYTES + 4 * 1024;
/** Per user, per minute. Reads cover the app's polling from several tabs; writes cover autosave. */
export const READS_PER_MINUTE = 600;
export const WRITES_PER_MINUTE = 240;

/** Sent by the website on every call: the id of the user the page believes is signed in. */
export const ACCOUNT_HEADER = 'x-firesight-user';

const COLLECTION = '/api/plans';
const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const etag = (meta: PlanMeta) => ({ etag: `"${meta.rev}"` });

/** The plan id a request names, '' for the collection, or null for a path that is not ours. */
export function planIdOf(url: URL): string | null {
  const viaRewrite = url.searchParams.get('id');
  if (url.pathname === COLLECTION || url.pathname === `${COLLECTION}/`) return viaRewrite ?? '';
  if (!url.pathname.startsWith(`${COLLECTION}/`)) return null;
  const rest = url.pathname.slice(COLLECTION.length + 1);
  return rest.includes('/') ? null : decodeURIComponent(rest);
}

/** `If-Match: 3`, `"3"` or `W/"3"` → 3. Undefined when absent, null when unusable. */
export function expectedRev(request: Request): number | null | undefined {
  const raw = request.headers.get('if-match');
  if (raw === null) return undefined;
  const match = /^(?:W\/)?"?(\d{1,9})"?$/.exec(raw.trim());
  const rev = match ? Number(match[1]) : 0;
  return rev >= 1 ? rev : null;
}

function planProblem(error: PlanError): Response {
  switch (error.code) {
    case 'invalid_plan':
      return problem(400, error.code, error.message);
    case 'too_large':
      return problem(413, error.code, error.message);
    case 'too_many_plans':
      return problem(409, error.code, error.message);
    case 'not_found':
      return problem(404, error.code, error.message);
    case 'conflict':
      return problem(409, error.code, error.message, error.current ? { current: error.current } : {});
  }
}

export async function handle(request: Request, env: Env, deps: Deps = {}): Promise<Response> {
  const state = deps.state ?? runtimeFor(env);
  if (state.status === 'off') return notConfigured();
  if (state.status === 'misconfigured') return misconfigured(state.message);
  const runtime = state.runtime;
  const now = deps.now ?? Date.now;

  const url = new URL(request.url);
  const id = planIdOf(url);
  if (id === null) return problem(404, 'not_found', 'There is nothing at this address.');
  const method = request.method;
  const allowed = id === '' ? ['GET', 'POST'] : ['GET', 'PUT', 'DELETE'];
  if (!allowed.includes(method)) return problem(405, 'method_not_allowed', `Use ${allowed.join(', ')}.`, {}, { allow: allowed.join(', ') });
  const write = method !== 'GET';

  // Before the session is even looked at: a cross-site request gets nothing to probe.
  if (write ? !fromThisSite(request, runtime.config) : !notFromAnotherSite(request, runtime.config)) {
    return problem(403, 'forbidden_origin', 'Requests are accepted only from this site.');
  }

  try {
    const user = await sessionUser(runtime, request);
    if (!user) return problem(401, 'unauthenticated', 'Sign in to use plans in your account.');
    // The page names the account it believes it is acting for. When another tab has
    // signed in as someone else since, the cookie and the page disagree: serving the
    // request would show one person's plans to a page that thinks it is the other's, or
    // write into the wrong account. A page that sends no header is served as before.
    const believed = request.headers.get(ACCOUNT_HEADER);
    if (believed !== null && believed !== user.id) return problem(401, 'account_changed', 'A different account is signed in on this browser now. Reload the page.');

    const rate = await consumeRate(runtime.db, userRateKey(write ? 'plans:w' : 'plans:r', user.id), write ? WRITES_PER_MINUTE : READS_PER_MINUTE, 60, now());
    if (!rate.allowed) return problem(429, 'rate_limited', 'Too many requests. Try again shortly.', {}, { 'retry-after': String(rate.retryAfter) });

    if (id === '') {
      if (method === 'GET') {
        if (url.searchParams.get('export') === '1') {
          const plans = await runtime.plans.exportPlans(user.id);
          return json(200, { exported_at: new Date(now()).toISOString(), plans }, { 'content-disposition': 'attachment; filename="firesight-plans.json"' });
        }
        return json(200, { plans: await runtime.plans.listPlans(user.id) });
      }
      const body = await readJson(request, MAX_BODY_BYTES);
      if (!body.ok) return body.response;
      if (!isRecord(body.value)) return problem(400, 'bad_request', 'The body must be { plan, name? }.');
      if (body.value.name !== undefined && typeof body.value.name !== 'string') return problem(400, 'bad_request', 'name must be text.');
      const meta = await runtime.plans.createPlan(user.id, body.value.plan, 'web', body.value.name);
      return json(201, meta, { location: `${COLLECTION}/${meta.id}`, ...etag(meta) });
    }

    if (method === 'GET') {
      const stored = await runtime.plans.getPlan(user.id, id);
      if (!stored) return problem(404, 'not_found', 'There is no such plan.');
      return json(200, stored, etag(stored));
    }

    if (method === 'DELETE') {
      if (!(await runtime.plans.deletePlan(user.id, id))) return problem(404, 'not_found', 'There is no such plan.');
      return new Response(null, { status: 204, headers: { 'cache-control': 'no-store' } });
    }

    // PUT. The rev the client last saw is required, so one writer cannot silently undo another.
    const rev = expectedRev(request);
    if (rev === undefined) return problem(428, 'precondition_required', 'Send the rev you last read in an If-Match header.');
    if (rev === null) return problem(400, 'bad_request', 'If-Match must be the plan rev, a positive integer.');
    const body = await readJson(request, MAX_BODY_BYTES);
    if (!body.ok) return body.response;
    if (!isRecord(body.value)) return problem(400, 'bad_request', 'The body must be { plan, name? }.');
    if (body.value.name !== undefined && typeof body.value.name !== 'string') return problem(400, 'bad_request', 'name must be text.');
    const meta = await runtime.plans.updatePlan(user.id, id, body.value.plan, rev, 'web', body.value.name);
    return json(200, meta, etag(meta));
  } catch (error) {
    if (error instanceof PlanError) return planProblem(error);
    if (error instanceof DecryptError) return problem(500, 'unreadable', 'A stored plan could not be decrypted.');
    deps.report?.(error);
    return problem(500, 'server_error', 'Something went wrong on the server.');
  }
}

export default {
  fetch(request: Request): Promise<Response> {
    return handle(request, process.env);
  },
};
