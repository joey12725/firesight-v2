// The Better Auth client: sign-in, API keys and the OAuth consent step, against this
// site's own /api/auth. It is only imported by the account pages, which load on demand,
// so nobody who never opens them downloads it.
//
// `oauthProviderClient` carries a pending authorisation through sign-in: when an AI
// tool sends the browser to /login or /consent, the signed query in the address goes
// back to the server with the next request, and the answer says where to go next. The
// client follows that answer by itself (an answer of `{ redirect: true, url }` makes it
// navigate), so the pages do not navigate a second time.
import { apiKeyClient } from '@better-auth/api-key/client';
import { oauthProviderClient } from '@better-auth/oauth-provider/client';
import { createAuthClient } from 'better-auth/react';

export const authClient = createAuthClient({ plugins: [apiKeyClient(), oauthProviderClient()] });

/** The message of a failed auth call, in words for the person. */
export function authMessage(error: { message?: string; status?: number; code?: string } | null | undefined, fallback: string): string {
  if (!error) return fallback;
  if (error.status === 429) return 'Too many attempts. Wait a minute and try again.';
  if (error.status === 503) return 'Accounts are not available on this site right now.';
  if (error.status === 0 || error.status === undefined) return error.message || 'The server could not be reached.';
  return error.message || fallback;
}
