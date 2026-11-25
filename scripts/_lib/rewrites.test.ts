import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { REPO_ROOT } from './dev-runtime.js';
import { applyRewrites } from './rewrites.js';
import type { Rewrite } from './rewrites.js';

const vercel = JSON.parse(readFileSync(join(REPO_ROOT, 'vercel.json'), 'utf8')) as { rewrites: Rewrite[]; functions: Record<string, unknown>; installCommand: string };
const route = (path: string, search = '') => applyRewrites(vercel.rewrites, path, search);

describe('vercel.json', () => {
  it('routes the account paths to their functions, carrying what the function needs in the query', () => {
    expect(route('/api/config')).toBe('/api/wellknown?doc=config');
    expect(route('/api/auth/sign-in/email')).toBe('/api/auth?fs_path=sign-in/email');
    expect(route('/api/auth/get-session')).toBe('/api/auth?fs_path=get-session');
    expect(route('/api/auth/oauth2/authorize', '?client_id=abc&state=x')).toBe('/api/auth?fs_path=oauth2/authorize&client_id=abc&state=x');
    expect(route('/api/auth/.well-known/openid-configuration')).toBe('/api/auth?fs_path=.well-known/openid-configuration');
    expect(route('/api/plans/pl_abc')).toBe('/api/plans?id=pl_abc');
    expect(route('/.well-known/oauth-protected-resource')).toBe('/api/wellknown?doc=oauth-protected-resource');
    expect(route('/.well-known/oauth-protected-resource/api/mcp')).toBe('/api/wellknown?doc=oauth-protected-resource&suffix=api/mcp');
    expect(route('/.well-known/oauth-authorization-server/api/auth')).toBe('/api/wellknown?doc=oauth-authorization-server&suffix=api/auth');
    expect(route('/.well-known/openid-configuration/api/auth')).toBe('/api/wellknown?doc=openid-configuration&suffix=api/auth');
  });

  it('leaves functions that are addressed directly, and everything else, alone', () => {
    for (const path of ['/api/plans', '/api/chat', '/api/mcp', '/api/plans/a/b', '/.well-known/security.txt', '/login']) expect(route(path), path).toBeNull();
  });

  it('puts every API rule before the single-page-app fallback', () => {
    const spa = vercel.rewrites.findIndex((r) => r.destination === '/index.html');
    expect(spa).toBe(vercel.rewrites.length - 1);
    // The fallback itself must not swallow API or discovery paths.
    const fallback = new RegExp(`^${vercel.rewrites[spa]?.source}$`);
    for (const path of ['/api/plans', '/api/auth/get-session', '/.well-known/oauth-protected-resource', '/assets/x']) expect(fallback.test(path), path).toBe(false);
    for (const path of ['/login', '/consent', '/account', '/p/pl_abc', '/']) expect(fallback.test(path), path).toBe(true);
  });

  it('declares every function it routes to, installs the root package, and stays within six functions', () => {
    const targets = new Set(vercel.rewrites.map((r) => r.destination.split('?')[0]).filter((d) => d?.startsWith('/api/')));
    for (const target of targets) expect(Object.keys(vercel.functions), target).toContain(`${target?.slice(1)}.ts`);
    expect(Object.keys(vercel.functions).length).toBeLessThanOrEqual(6);
    expect(vercel.installCommand).toMatch(/^npm install --no-audit --no-fund && npm install --prefix web /);
  });
});

describe('applyRewrites', () => {
  const rules: Rewrite[] = [
    { source: '/a/:id', destination: '/fn?id=:id' },
    { source: '/b/:rest*', destination: '/fn?rest=:rest*' },
    { source: '/c/:id/:ident', destination: '/fn?x=:ident&y=:id' },
    { source: '/((?!api/)[^.]*)', destination: '/index.html' },
  ];

  it('matches one segment for :name and the remainder for :name*', () => {
    expect(applyRewrites(rules, '/a/1', '')).toBe('/fn?id=1');
    expect(applyRewrites(rules, '/a/1/2', '')).toBeNull();
    expect(applyRewrites(rules, '/a/', '')).toBeNull();
    expect(applyRewrites(rules, '/b/1/2/3', '')).toBe('/fn?rest=1/2/3');
    expect(applyRewrites(rules, '/b/', '')).toBe('/fn?rest=');
    expect(applyRewrites(rules, '/c/1/2', '?q=3')).toBe('/fn?x=2&y=1&q=3');
    expect(applyRewrites(rules, '/anything', '')).toBeNull();
  });
});
