// The Better Auth instance: accounts, sessions, API keys and the OAuth 2.1
// authorisation server that remote MCP clients sign in through.
//
// Three credentials exist and they are deliberately not interchangeable:
//   session cookie       the website's own API (/api/plans, /api/auth/*)
//   API key (fs_…)       the MCP endpoint, sent as a bearer token
//   OAuth access token   the MCP endpoint, a JWT whose audience is the MCP URL
// Nothing here turns an API key or an access token into a browser session.

import { apiKey } from '@better-auth/api-key';
import { cimd } from '@better-auth/cimd';
import { fetchClientMetadataResource } from '@better-auth/cimd/node';
import { mcp } from '@better-auth/mcp';
import { betterAuth } from 'better-auth';
import type { BetterAuthOptions } from 'better-auth';
import { APIError, createAuthMiddleware, getSessionFromCtx } from 'better-auth/api';
import { jwt } from 'better-auth/plugins';
import type { Db, SqlPool } from './db.js';
import { AUTH_BASE_PATH, OAUTH_SCOPES, PLAN_SCOPES } from './env.js';
import type { ServerConfig } from './env.js';
import { forgetUserRates } from './ratelimit.js';

export const MIN_PASSWORD_LENGTH = 12;

/**
 * API keys come in two kinds, chosen at creation with `configId`. The kind fixes the
 * key's permissions on the server: a browser cannot ask for permissions directly, and
 * cannot change them afterwards. An unknown or missing `configId` gets the read-only
 * kind, so a mistake fails safe.
 */
export const API_KEY_READ_ONLY = 'default';
export const API_KEY_READ_WRITE = 'read-write';
export const API_KEY_PREFIX = 'fs_';

/** Requests per key per minute, across every instance (the counter lives in the key's row). */
const API_KEY_RATE = { enabled: true, timeWindow: 60_000, maxRequests: 120 };

/**
 * The OAuth plugin's client-management endpoints. They let any signed-in user create and
 * edit client rows, and sign-up is open, so leaving them on would be client registration
 * by another door whatever FIRESIGHT_OAUTH_DCR says. Nothing on this site calls them:
 * clients come from metadata documents, or from /oauth2/register when the operator has
 * turned that on. They answer 404.
 */
export const OAUTH_CLIENT_MANAGEMENT_PATHS = ['/oauth2/create-client', '/oauth2/update-client', '/oauth2/delete-client', '/oauth2/client/rotate-secret', '/oauth2/client/rotate-keys'];

export type AuthLog = (level: 'debug' | 'info' | 'warn' | 'error', message: string) => void;

export interface AuthDeps {
  pool: SqlPool;
  db: Db;
  /**
   * Where the library's own diagnostics go. Unset in production: this server writes no
   * logs, so that neither credentials nor plan contents can end up in one. The local
   * dev server passes a sink that prints the message text only.
   */
  log?: AuthLog;
}

function options(config: ServerConfig, deps: AuthDeps) {
  const secure = config.origin.startsWith('https:');
  const socialProviders: NonNullable<BetterAuthOptions['socialProviders']> = {};
  if (config.social.github) socialProviders.github = config.social.github;
  if (config.social.google) socialProviders.google = config.social.google;

  const keyKind = (configId: string, permissions: Record<string, string[]>) => ({
    configId,
    defaultPrefix: API_KEY_PREFIX,
    requireName: true,
    maximumNameLength: 64,
    rateLimit: API_KEY_RATE,
    permissions: { defaultPermissions: permissions },
    // A session is never synthesised from a key: keys are for the MCP endpoint only.
    enableSessionForAPIKeys: false,
  });

  return {
    appName: 'FIRESight',
    baseURL: config.origin,
    basePath: AUTH_BASE_PATH,
    secret: config.secret,
    // Better Auth recognises a `pg` pool by shape; the embedded dev database has the same shape.
    database: deps.pool as unknown as NonNullable<BetterAuthOptions['database']>,
    trustedOrigins: [config.origin, ...config.extraOrigins],
    // '/token' is the JWT plugin's own session-token endpoint. Access tokens come from /oauth2/token only.
    disabledPaths: ['/token', ...OAUTH_CLIENT_MANAGEMENT_PATHS],
    telemetry: { enabled: false },
    logger: deps.log ? { level: 'warn' as const, disableColors: true, log: (level: Parameters<AuthLog>[0], message: string) => deps.log?.(level, message) } : { disabled: true },
    emailAndPassword: {
      enabled: true,
      minPasswordLength: MIN_PASSWORD_LENGTH,
      maxPasswordLength: 256,
      // There is no email sender, so no verification step and no password reset yet.
      requireEmailVerification: false,
      autoSignIn: true,
    },
    socialProviders,
    user: {
      deleteUser: {
        enabled: true,
        // Rows that hang off the user without a foreign key would otherwise outlive the
        // account. Plans and the user's data key go by `on delete cascade`.
        beforeDelete: async (user) => {
          await deps.db.query('delete from "apikey" where "referenceId" = $1', [user.id]);
          await forgetUserRates(deps.db, user.id);
        },
      },
    },
    session: {
      // No cookie cache: a revoked session stops working on the next request.
      cookieCache: { enabled: false },
    },
    rateLimit: {
      enabled: config.rateLimit,
      // Shared by every function instance. An in-memory counter would reset on each cold start.
      storage: 'database',
      window: 60,
      max: 120,
      customRules: {
        '/sign-in/email': { window: 60, max: 10 },
        '/sign-up/email': { window: 300, max: 10 },
        '/delete-user': { window: 300, max: 5 },
        '/api-key/create': { window: 60, max: 10 },
        '/oauth2/register': { window: 300, max: 10 },
        '/oauth2/token': { window: 60, max: 60 },
      },
    },
    advanced: {
      cookiePrefix: 'firesight',
      useSecureCookies: secure,
      // Stated, not defaulted: the library turns both checks off by itself when
      // NODE_ENV is "test", and nothing about this server should depend on NODE_ENV.
      disableCSRFCheck: false,
      disableOriginCheck: false,
      // Vercel overwrites this header with the address it saw; the dev server does the same.
      ipAddress: { ipAddressHeaders: ['x-forwarded-for'] },
    },
    hooks: {
      before: createAuthMiddleware(async (ctx) => {
        // Deleting an account that has a password takes the password. The library asks
        // only for a session under a day old when none is sent, which would let anyone at
        // an unlocked browser delete the account. A user who has only ever signed in
        // through a provider has no password to give, and keeps the library's rule.
        if (ctx.path === '/delete-user') {
          const body: Record<string, unknown> | undefined = ctx.body;
          if (typeof body?.password === 'string' && body.password !== '') return;
          const session = await getSessionFromCtx(ctx);
          if (!session) return;
          const credential = await deps.db.query(`select 1 from account where "userId" = $1 and "providerId" = 'credential' and password is not null limit 1`, [session.user.id]);
          if (credential.rows.length > 0) throw new APIError('BAD_REQUEST', { code: 'PASSWORD_REQUIRED', message: 'Enter your password to delete the account.' });
          return;
        }
        // The key endpoints look a key up within one kind and default to the read-only
        // kind, so revoking a read-write key by id alone would answer "not found". The
        // kind is a property of the key, not something the caller should have to restate:
        // fill it in from the key's row. Ownership is still checked by the endpoint itself.
        if (ctx.path !== '/api-key/delete' && ctx.path !== '/api-key/update' && ctx.path !== '/api-key/get') return;
        const source: Record<string, unknown> | undefined = ctx.path === '/api-key/get' ? ctx.query : ctx.body;
        const keyId = ctx.path === '/api-key/get' ? source?.id : source?.keyId;
        if (!source || typeof keyId !== 'string' || source.configId !== undefined) return;
        const found = await deps.db.query<{ configId: string | null }>('select "configId" from "apikey" where id = $1', [keyId]);
        const configId = found.rows[0]?.configId;
        if (!configId) return;
        return { context: ctx.path === '/api-key/get' ? { ...ctx, query: { ...source, configId } } : { ...ctx, body: { ...source, configId } } };
      }),
      // Disconnecting an app must cut it off, not just forget that the user once agreed.
      // The library removes the consent row and leaves the app's refresh tokens working,
      // so they are removed here: every token of this user's whose client no longer has
      // their consent. (Access tokens already issued are checked against the consent row
      // when they are used: see `oauthPrincipal`.)
      after: createAuthMiddleware(async (ctx) => {
        if (ctx.path !== '/oauth2/delete-consent') return;
        const session = await getSessionFromCtx(ctx);
        if (!session) return;
        await revokeUnconsentedTokens(deps.db, session.user.id);
      }),
    },
    plugins: [
      // Signs OAuth access tokens. Sessions stay opaque cookies, hence no JWT response header.
      jwt({ disableSettingJwtHeader: true }),
      mcp({
        resource: config.mcpUrl,
        loginPage: '/login',
        consentPage: '/consent',
        scopes: OAUTH_SCOPES,
        // A person is always behind a token here; there are no machine-to-machine clients.
        grantTypes: ['authorization_code', 'refresh_token'],
        // Dynamic Client Registration lets anyone on the internet create client rows, so it is
        // off unless the operator needs it for a client that cannot use metadata documents.
        allowDynamicClientRegistration: config.dcr,
        allowUnauthenticatedClientRegistration: config.dcr,
        clientRegistrationDefaultScopes: [...PLAN_SCOPES, 'offline_access'],
        clientRegistrationAllowedScopes: OAUTH_SCOPES,
        // A client that registers loopback redirect URIs is a native app (Claude Code, an
        // editor), not a website that must use https.
        clientRegistrationDefaultApplicationType: 'infer',
      }),
      // Client ID Metadata Documents: an MCP client identifies itself with an https URL and
      // the server fetches its metadata, with DNS pinned and private addresses refused.
      cimd({ fetchClientMetadataResource, metadataProfile: 'mcp-2026-07-28' }),
      apiKey([keyKind(API_KEY_READ_ONLY, { plans: ['read'] }), keyKind(API_KEY_READ_WRITE, { plans: ['read', 'write'] })]),
    ],
  } satisfies BetterAuthOptions;
}

/**
 * Removes a user's OAuth tokens for every client they no longer consent to. Idempotent,
 * and a no-op while consent stands, so it is safe to run after any consent change.
 */
export async function revokeUnconsentedTokens(db: Db, userId: string): Promise<void> {
  for (const table of ['oauthRefreshToken', 'oauthAccessToken']) {
    await db.query(
      `delete from "${table}" t where t."userId" = $1
         and not exists (select 1 from "oauthConsent" c where c."userId" = t."userId" and c."clientId" = t."clientId")`,
      [userId],
    );
  }
}

export function createAuth(config: ServerConfig, deps: AuthDeps) {
  return betterAuth(options(config, deps));
}

export type Auth = ReturnType<typeof createAuth>;
