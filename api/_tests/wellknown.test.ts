import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { handle as handleAuth } from '../auth.js';
import { handle as handlePlans } from '../plans.js';
import { handle, protectedResourceMetadata, publicConfig, requestedDoc } from '../wellknown.js';
import type { SqlPool } from '../_lib/db.js';
import { readConfig } from '../_lib/env.js';
import { createRuntime, runtimeFor } from '../_lib/runtime.js';
import type { RuntimeState } from '../_lib/runtime.js';
import { Browser, ORIGIN, startRuntime, testEnv } from './harness.js';
import type { TestRuntime } from './harness.js';

let t: TestRuntime;
let dcr: TestRuntime;

beforeAll(async () => {
  [t, dcr] = await Promise.all([startRuntime(), startRuntime({ env: { FIRESIGHT_OAUTH_DCR: '1', GITHUB_CLIENT_ID: 'id', GITHUB_CLIENT_SECRET: 'secret' } })]);
});

afterAll(() => Promise.all([t.close(), dcr.close()]));

const get = async <T = Record<string, unknown>>(path: string, state: RuntimeState = t.state, init: RequestInit = {}) => {
  const response = await handle(new Request(new URL(path, ORIGIN), init), {}, { state });
  const text = await response.text();
  return { status: response.status, headers: response.headers, body: (text ? JSON.parse(text) : null) as T };
};

describe('GET /api/config', () => {
  it('describes a deployment with accounts', async () => {
    const reply = await get('/api/config');
    expect(reply.status).toBe(200);
    expect(reply.body).toEqual({ accounts: true, mcp_url: 'https://firesight.test/api/mcp', social: [], dcr: false });
    expect(reply.headers.get('cache-control')).toBe('public, max-age=60');
    expect((await get('/api/config', dcr.state)).body).toEqual({ accounts: true, mcp_url: 'https://firesight.test/api/mcp', social: ['github'], dcr: true });
    // The rewritten form.
    expect((await get('/api/wellknown?doc=config')).body).toMatchObject({ accounts: true });
  });

  it('says accounts are off when they are not configured or are misconfigured, and never fails', async () => {
    const off = { accounts: false, mcp_url: null, social: [], dcr: false };
    expect((await get('/api/config', { status: 'off' })).body).toEqual(off);
    expect((await get('/api/config', { status: 'misconfigured', message: 'PLAN_KEK must be…' })).body).toEqual(off);
    expect(publicConfig({ status: 'off' })).toEqual(off);
    // Straight from an empty environment, as on a deployment with no database.
    const response = await handle(new Request(`${ORIGIN}/api/config`), {});
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual(off);
  });

  it('answers without touching the database', async () => {
    const config = readConfig(testEnv());
    if (!config) throw new Error('No config.');
    const dead = () => Promise.reject(new Error('the database is down'));
    const pool: SqlPool = { connect: dead, query: dead, end: () => Promise.resolve() };
    const state: RuntimeState = { status: 'ready', runtime: createRuntime(config, pool) };
    expect((await get('/api/config', state)).body).toMatchObject({ accounts: true });
    const prm = await get('/.well-known/oauth-protected-resource/api/mcp', state);
    expect(prm.status).toBe(200);
    expect(prm.body).toMatchObject({ resource: 'https://firesight.test/api/mcp' });
  });

  it('carries no secret', async () => {
    const text = JSON.stringify((await get('/api/config', dcr.state)).body);
    expect(text).not.toContain(dcr.config.secret);
    expect(text).not.toContain('secret');
  });
});

describe('the unconfigured deployment', () => {
  it('answers 503 JSON from every account endpoint and leaves /api/config working', async () => {
    const env = {};
    expect(runtimeFor(env)).toEqual({ status: 'off' });
    const calls: Array<[string, Promise<Response>]> = [
      ['auth', handleAuth(new Request(`${ORIGIN}/api/auth/get-session`), env)],
      ['auth post', handleAuth(new Request(`${ORIGIN}/api/auth/sign-in/email`, { method: 'POST', body: '{}' }), env)],
      ['plans', handlePlans(new Request(`${ORIGIN}/api/plans`), env)],
      ['plan', handlePlans(new Request(`${ORIGIN}/api/plans/pl_x`, { method: 'DELETE' }), env)],
      ['prm', handle(new Request(`${ORIGIN}/.well-known/oauth-protected-resource`), env)],
      ['as', handle(new Request(`${ORIGIN}/.well-known/oauth-authorization-server/api/auth`), env)],
      ['oidc', handle(new Request(`${ORIGIN}/api/auth/.well-known/openid-configuration`), env)],
    ];
    for (const [name, call] of calls) {
      const response = await call;
      expect(response.status, name).toBe(503);
      expect(response.headers.get('content-type'), name).toBe('application/json');
      expect(await response.json(), name).toMatchObject({ error: 'accounts_unavailable' });
    }
  });

  it('reports a misconfiguration by naming the variable, not its value', async () => {
    const env = testEnv({ PLAN_KEK: 'definitely-not-a-key' });
    const state = runtimeFor(env);
    expect(state.status).toBe('misconfigured');
    const response = await handlePlans(new Request(`${ORIGIN}/api/plans`), env);
    expect(response.status).toBe(503);
    const body = (await response.json()) as { error: string; message: string };
    expect(body.error).toBe('accounts_misconfigured');
    expect(body.message).toContain('PLAN_KEK');
    expect(body.message).not.toContain('definitely-not-a-key');
    expect((await handleAuth(new Request(`${ORIGIN}/api/auth/get-session`), env)).status).toBe(503);
  });

  it('builds the runtime once per environment', () => {
    const env = testEnv();
    const first = runtimeFor(env);
    expect(first.status).toBe('ready');
    expect(runtimeFor(env)).toBe(first);
  });
});

describe('OAuth discovery documents', () => {
  it('finds the document in the original path or in the rewrite query', () => {
    const doc = (path: string) => requestedDoc(new URL(path, ORIGIN));
    expect(doc('/api/config')).toEqual({ doc: 'config', suffix: '' });
    expect(doc('/.well-known/oauth-protected-resource')).toEqual({ doc: 'oauth-protected-resource', suffix: '' });
    expect(doc('/.well-known/oauth-protected-resource/api/mcp')).toEqual({ doc: 'oauth-protected-resource', suffix: '/api/mcp' });
    expect(doc('/api/wellknown?doc=oauth-protected-resource&suffix=api/mcp')).toEqual({ doc: 'oauth-protected-resource', suffix: '/api/mcp' });
    expect(doc('/api/wellknown?doc=oauth-authorization-server&suffix=')).toEqual({ doc: 'oauth-authorization-server', suffix: '' });
    expect(doc('/api/auth/.well-known/openid-configuration')).toEqual({ doc: 'openid-configuration', suffix: '/api/auth' });
    expect(doc('/api/wellknown')).toBeNull();
  });

  it('serves protected resource metadata (RFC 9728) for the MCP endpoint', async () => {
    for (const path of ['/.well-known/oauth-protected-resource/api/mcp', '/.well-known/oauth-protected-resource', '/api/wellknown?doc=oauth-protected-resource&suffix=api/mcp', '/api/wellknown?doc=oauth-protected-resource']) {
      const reply = await get(path);
      expect(reply.status, path).toBe(200);
      expect(reply.body).toEqual({
        resource: 'https://firesight.test/api/mcp',
        authorization_servers: ['https://firesight.test/api/auth'],
        bearer_methods_supported: ['header'],
        dpop_signing_alg_values_supported: ['EdDSA', 'ES256', 'ES512', 'PS256', 'RS256'],
        // Not offline_access or the OpenID scopes: those are the authorisation server's.
        scopes_supported: ['plans:read', 'plans:write'],
      });
      expect(reply.headers.get('access-control-allow-origin')).toBe('*');
      expect(reply.headers.get('cache-control')).toContain('max-age=300');
    }
  });

  it("matches the auth library's own protected resource document exactly", async () => {
    const own = await t.runtime.auth.handler(new Request(`${ORIGIN}/.well-known/oauth-protected-resource/api/mcp`));
    expect(own.status).toBe(200);
    expect(protectedResourceMetadata(t.config)).toEqual(await own.json());
  });

  it('serves authorisation server metadata (RFC 8414) with what MCP clients look for', async () => {
    for (const path of ['/.well-known/oauth-authorization-server/api/auth', '/.well-known/oauth-authorization-server', '/api/wellknown?doc=oauth-authorization-server&suffix=api/auth']) {
      const { status, body } = await get<Record<string, unknown>>(path);
      expect(status, path).toBe(200);
      expect(body).toMatchObject({
        issuer: 'https://firesight.test/api/auth',
        authorization_endpoint: 'https://firesight.test/api/auth/oauth2/authorize',
        token_endpoint: 'https://firesight.test/api/auth/oauth2/token',
        jwks_uri: 'https://firesight.test/api/auth/jwks',
        response_types_supported: ['code'],
        code_challenge_methods_supported: ['S256'],
        client_id_metadata_document_supported: true,
        authorization_response_iss_parameter_supported: true,
        grant_types_supported: ['authorization_code', 'refresh_token'],
        scopes_supported: ['openid', 'profile', 'email', 'offline_access', 'plans:read', 'plans:write'],
      });
      // Claude picks metadata-document registration only when public clients are allowed.
      expect(body.token_endpoint_auth_methods_supported).toContain('none');
      // Dynamic registration is off, so no registration endpoint is advertised.
      expect(body).not.toHaveProperty('registration_endpoint');
    }
  });

  it('advertises a registration endpoint only when dynamic registration is on', async () => {
    const { body } = await get('/.well-known/oauth-authorization-server/api/auth', dcr.state);
    expect(body.registration_endpoint).toBe('https://firesight.test/api/auth/oauth2/register');
    expect(body.client_id_metadata_document_supported).toBe(true);
  });

  it('serves OpenID configuration at the issuer and at the path-inserted form', async () => {
    for (const path of ['/api/auth/.well-known/openid-configuration', '/.well-known/openid-configuration/api/auth', '/.well-known/openid-configuration', '/api/wellknown?doc=openid-configuration&suffix=api/auth']) {
      const { status, body } = await get(path);
      expect(status, path).toBe(200);
      expect(body).toMatchObject({ issuer: 'https://firesight.test/api/auth', userinfo_endpoint: 'https://firesight.test/api/auth/oauth2/userinfo', subject_types_supported: ['public'] });
    }
  });

  it("matches the documents the auth library serves on its own routes", async () => {
    const own = await t.runtime.auth.handler(new Request(`${ORIGIN}/.well-known/oauth-authorization-server/api/auth`));
    expect((await get('/.well-known/oauth-authorization-server/api/auth')).body).toEqual(await own.json());
    const oidc = await t.runtime.auth.handler(new Request(`${ORIGIN}/api/auth/.well-known/openid-configuration`));
    expect((await get('/api/auth/.well-known/openid-configuration')).body).toEqual(await oidc.json());
    // And the auth function itself serves the issuer-relative form.
    const viaAuth = await new Browser(t.state).get('/api/auth/.well-known/openid-configuration');
    expect(viaAuth.status).toBe(200);
    expect(viaAuth.headers.get('cache-control')).toContain('public');
  });

  it('answers 404 for a document that is not ours, 405 for writes, and handles HEAD and preflight', async () => {
    expect((await get('/.well-known/oauth-authorization-server/other/issuer')).status).toBe(404);
    expect((await get('/.well-known/oauth-protected-resource/api/plans')).status).toBe(404);
    expect((await get('/.well-known/security.txt')).status).toBe(404);
    expect((await get('/api/wellknown?doc=nope')).status).toBe(404);
    expect((await get('/api/wellknown')).status).toBe(404);
    const post = await get('/.well-known/oauth-protected-resource', t.state, { method: 'POST' });
    expect(post.status).toBe(405);
    expect(post.headers.get('allow')).toBe('GET, HEAD, OPTIONS');
    const head = await handle(new Request(`${ORIGIN}/.well-known/oauth-protected-resource`, { method: 'HEAD' }), {}, { state: t.state });
    expect(head.status).toBe(200);
    expect(await head.text()).toBe('');
    const preflight = await handle(new Request(`${ORIGIN}/.well-known/oauth-authorization-server`, { method: 'OPTIONS', headers: { origin: 'https://inspector.example', 'access-control-request-headers': 'mcp-protocol-version' } }), {}, { state: t.state });
    expect(preflight.status).toBe(204);
    expect(preflight.headers.get('access-control-allow-origin')).toBe('*');
  });

  it('publishes signing keys at the advertised JWKS address', async () => {
    const browser = new Browser(t.state);
    const jwks = await browser.get<{ keys: Array<Record<string, string>> }>('/api/auth/jwks');
    expect(jwks.status).toBe(200);
    expect(jwks.body.keys.length).toBeGreaterThan(0);
    for (const key of jwks.body.keys) {
      expect(key.kid).toBeTruthy();
      // Public parts only.
      expect(key).not.toHaveProperty('d');
    }
  });
});
