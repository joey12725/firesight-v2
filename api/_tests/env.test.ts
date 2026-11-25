import { randomBytes } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import { ConfigError, publicOrigin, readConfig } from '../_lib/env.js';
import { testEnv } from './harness.js';

describe('readConfig', () => {
  it('is null (accounts off) when any of the database, the secret or the plan key is missing', () => {
    expect(readConfig({})).toBeNull();
    for (const missing of ['DATABASE_URL', 'BETTER_AUTH_SECRET', 'PLAN_KEK', 'BETTER_AUTH_URL']) {
      expect(readConfig({ ...testEnv(), [missing]: undefined }), missing).toBeNull();
      expect(readConfig({ ...testEnv(), [missing]: '  ' }), missing).toBeNull();
    }
  });

  it('derives the issuer and the MCP URL from the origin', () => {
    const config = readConfig(testEnv({ BETTER_AUTH_URL: 'https://example.com/' }));
    expect(config).toMatchObject({ origin: 'https://example.com', issuer: 'https://example.com/api/auth', mcpUrl: 'https://example.com/api/mcp', dcr: false, rateLimit: true, social: {} });
  });

  it('prefers the unpooled connection string for migrations', () => {
    expect(readConfig(testEnv())?.migrationUrl).toBe('postgres://embedded');
    expect(readConfig(testEnv({ DATABASE_URL_UNPOOLED: 'postgres://direct' }))?.migrationUrl).toBe('postgres://direct');
  });

  it('falls back to the Vercel production domain, in production only', () => {
    const env = { ...testEnv(), BETTER_AUTH_URL: undefined, VERCEL_PROJECT_PRODUCTION_URL: 'firesight.example.app' };
    expect(readConfig({ ...env, VERCEL_ENV: 'production' })?.origin).toBe('https://firesight.example.app');
    expect(readConfig({ ...env, VERCEL_ENV: 'preview' })).toBeNull();
    expect(publicOrigin({ BETTER_AUTH_URL: 'http://localhost:5173' })).toBe('http://localhost:5173');
  });

  it('is loud about a malformed origin, secret or key instead of silently turning accounts off', () => {
    expect(() => readConfig(testEnv({ BETTER_AUTH_URL: 'not a url' }))).toThrow(ConfigError);
    expect(() => readConfig(testEnv({ BETTER_AUTH_URL: 'http://example.com' }))).toThrow(/https/);
    expect(() => readConfig(testEnv({ BETTER_AUTH_URL: 'https://example.com/app' }))).toThrow(/bare origin/);
    expect(() => readConfig(testEnv({ BETTER_AUTH_SECRET: 'short' }))).toThrow(/32 characters/);
    expect(() => readConfig(testEnv({ PLAN_KEK: 'c2hvcnQ=' }))).toThrow(/32 random bytes/);
    expect(() => readConfig(testEnv({ PLAN_KEK: `${'!'.repeat(43)}=` }))).toThrow(/32 random bytes/);
    expect(() => readConfig(testEnv({ PLAN_KEK_VERSION: '0' }))).toThrow(ConfigError);
  });

  it('never puts a secret value in an error message', () => {
    const secret = 'tooshort-secret-value';
    try {
      readConfig(testEnv({ BETTER_AUTH_SECRET: secret }));
      expect.unreachable();
    } catch (error) {
      expect((error as Error).message).not.toContain(secret);
    }
  });

  it('accepts url-safe base64 keys and keeps retired keys by version', () => {
    const old = randomBytes(32);
    const config = readConfig(testEnv({ PLAN_KEK: randomBytes(32).toString('base64url'), PLAN_KEK_VERSION: '3', PLAN_KEK_V2: old.toString('base64') }));
    expect(config?.kekVersion).toBe(3);
    expect([...(config?.keks.keys() ?? [])].sort()).toEqual([2, 3]);
    expect(config?.keks.get(2)?.equals(old)).toBe(true);
  });

  it('offers a social provider only when both of its values are set', () => {
    expect(readConfig(testEnv({ GITHUB_CLIENT_ID: 'id' }))?.social).toEqual({});
    expect(readConfig(testEnv({ GITHUB_CLIENT_ID: 'id', GITHUB_CLIENT_SECRET: 's', GOOGLE_CLIENT_ID: 'gid', GOOGLE_CLIENT_SECRET: 'gs' }))?.social).toEqual({
      github: { clientId: 'id', clientSecret: 's' },
      google: { clientId: 'gid', clientSecret: 'gs' },
    });
  });

  it('turns dynamic client registration on only for exactly FIRESIGHT_OAUTH_DCR=1', () => {
    expect(readConfig(testEnv({ FIRESIGHT_OAUTH_DCR: '1' }))?.dcr).toBe(true);
    expect(readConfig(testEnv({ FIRESIGHT_OAUTH_DCR: 'true' }))?.dcr).toBe(false);
  });
});
