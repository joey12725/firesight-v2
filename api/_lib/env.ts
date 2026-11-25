// Server configuration, read once from the environment.
//
// Accounts are optional. A deployment with no database, no auth secret or no plan key
// has no accounts at all: `readConfig` returns null, `GET /api/config` says
// `{ accounts: false }`, every other account endpoint answers 503, and the static app
// behaves exactly as it did before accounts existed.

export type Env = Record<string, string | undefined>;

/** Scopes an MCP client can be granted on a user's plans. */
export const PLAN_SCOPES = ['plans:read', 'plans:write'] as const;
export type PlanScope = (typeof PLAN_SCOPES)[number];

/** Everything the authorisation server will issue. The first four are OpenID Connect's. */
export const OAUTH_SCOPES = ['openid', 'profile', 'email', 'offline_access', ...PLAN_SCOPES];

export const AUTH_BASE_PATH = '/api/auth';
export const MCP_PATH = '/api/mcp';

export interface SocialProvider {
  clientId: string;
  clientSecret: string;
}

export interface ServerConfig {
  /** The site's public origin, no trailing slash. Issuer, cookie scope and CSRF checks hang off it. */
  origin: string;
  /** Other origins allowed to call the cookie-authenticated API (the operator's own, e.g. a staging host). */
  extraOrigins: string[];
  /** The OAuth issuer: `<origin>/api/auth`. */
  issuer: string;
  /** The MCP endpoint, which is also the OAuth resource identifier tokens are bound to. */
  mcpUrl: string;
  /** Pooled connection string, for serving requests. */
  databaseUrl: string;
  /** Direct connection string, for migrations. Falls back to the pooled one. */
  migrationUrl: string;
  secret: string;
  /** Key-encryption keys by version. `kekVersion` is the one new data keys are wrapped with. */
  keks: Map<number, Buffer>;
  kekVersion: number;
  social: { github?: SocialProvider; google?: SocialProvider };
  /** Dynamic Client Registration, for MCP clients that cannot use Client ID Metadata Documents. */
  dcr: boolean;
  /** Always true from the environment. Tests that are not about rate limiting turn it off. */
  rateLimit: boolean;
}

export class ConfigError extends Error {}

const trimmed = (value: string | undefined): string | undefined => {
  const text = value?.trim();
  return text ? text : undefined;
};

function parseOrigin(raw: string, name: string): string {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw new ConfigError(`${name} is not a URL.`);
  }
  if (url.protocol !== 'https:' && url.protocol !== 'http:') throw new ConfigError(`${name} must be an http(s) URL.`);
  if (url.pathname !== '/' || url.search || url.hash || url.username || url.password) throw new ConfigError(`${name} must be a bare origin such as https://example.com.`);
  const loopback = url.hostname === 'localhost' || url.hostname === '127.0.0.1' || url.hostname === '[::1]';
  if (url.protocol === 'http:' && !loopback) throw new ConfigError(`${name} must use https, except on localhost.`);
  return url.origin;
}

function parseKek(raw: string, name: string): Buffer {
  // 32 bytes are 43 base64 characters and one pad. Buffer.from skips characters outside
  // the alphabet instead of failing, so the shape is checked first.
  const key = /^[A-Za-z0-9+/_-]{43}=?$/.test(raw) ? Buffer.from(raw, 'base64') : Buffer.alloc(0);
  if (key.length !== 32) throw new ConfigError(`${name} must be 32 random bytes in base64 (openssl rand -base64 32).`);
  return key;
}

/**
 * The site's public origin. `BETTER_AUTH_URL` when set; otherwise, on Vercel, the
 * project's production domain. Preview deployments therefore have no working accounts
 * unless they are given a `BETTER_AUTH_URL` of their own, which is deliberate: tokens
 * and cookies are bound to one origin.
 */
export function publicOrigin(env: Env): string | null {
  const explicit = trimmed(env.BETTER_AUTH_URL);
  if (explicit) return parseOrigin(explicit, 'BETTER_AUTH_URL');
  const vercel = trimmed(env.VERCEL_PROJECT_PRODUCTION_URL);
  if (vercel && env.VERCEL_ENV === 'production') return parseOrigin(`https://${vercel}`, 'VERCEL_PROJECT_PRODUCTION_URL');
  return null;
}

/**
 * The configuration, or null when accounts are not set up on this deployment.
 * Throws `ConfigError` when they are set up wrongly (a malformed key or origin):
 * that is an operator mistake and should be loud, not a silent "no accounts".
 */
export function readConfig(env: Env): ServerConfig | null {
  const databaseUrl = trimmed(env.DATABASE_URL);
  const secret = trimmed(env.BETTER_AUTH_SECRET);
  const kek = trimmed(env.PLAN_KEK);
  if (!databaseUrl || !secret || !kek) return null;
  const origin = publicOrigin(env);
  if (!origin) return null;
  if (secret.length < 32) throw new ConfigError('BETTER_AUTH_SECRET must be at least 32 characters (openssl rand -base64 32).');

  const kekVersion = Number(trimmed(env.PLAN_KEK_VERSION) ?? '1');
  if (!Number.isInteger(kekVersion) || kekVersion < 1) throw new ConfigError('PLAN_KEK_VERSION must be a positive integer.');
  const keks = new Map<number, Buffer>([[kekVersion, parseKek(kek, 'PLAN_KEK')]]);
  // Retired keys stay readable as PLAN_KEK_V<n> until every data key has been rewrapped.
  for (const [name, value] of Object.entries(env)) {
    const match = /^PLAN_KEK_V(\d+)$/.exec(name);
    const text = trimmed(value);
    if (!match || !text) continue;
    const version = Number(match[1]);
    if (version !== kekVersion) keks.set(version, parseKek(text, name));
  }

  const social: ServerConfig['social'] = {};
  const githubId = trimmed(env.GITHUB_CLIENT_ID);
  const githubSecret = trimmed(env.GITHUB_CLIENT_SECRET);
  if (githubId && githubSecret) social.github = { clientId: githubId, clientSecret: githubSecret };
  const googleId = trimmed(env.GOOGLE_CLIENT_ID);
  const googleSecret = trimmed(env.GOOGLE_CLIENT_SECRET);
  if (googleId && googleSecret) social.google = { clientId: googleId, clientSecret: googleSecret };

  const extraOrigins = (env.FIRESIGHT_ALLOWED_ORIGINS ?? '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
    .map((o) => parseOrigin(o, 'FIRESIGHT_ALLOWED_ORIGINS'));

  return {
    origin,
    extraOrigins,
    issuer: `${origin}${AUTH_BASE_PATH}`,
    mcpUrl: `${origin}${MCP_PATH}`,
    databaseUrl,
    migrationUrl: trimmed(env.DATABASE_URL_UNPOOLED) ?? databaseUrl,
    secret,
    keks,
    kekVersion,
    social,
    dcr: env.FIRESIGHT_OAUTH_DCR === '1',
    rateLimit: true,
  };
}
