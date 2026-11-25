// The MCP server itself: one `McpServer` per request, built for the caller who made it.
//
// Nothing is kept between requests (no sessions), which is what a serverless function
// needs and what the 2026-07-28 revision of the protocol expects. Clients that still
// speak a 2025 revision are served the same tools through the SDK's stateless fallback.

import { McpServer, createMcpHandler } from '@modelcontextprotocol/server';
import type { AuthInfo } from '@modelcontextprotocol/server';
import type { Principal } from '../credentials.js';
import type { Runtime } from '../runtime.js';
import { SERVER_INSTRUCTIONS } from './guide.js';
import { DEFAULT_LIMITS, registerTools } from './tools.js';
import type { McpLimits } from './tools.js';

export const SERVER_NAME = 'firesight';
/** Kept equal to the version in package.json by a test. */
export const SERVER_VERSION = '2.0.0-alpha.1';
/**
 * The largest request body. A plan is at most 256 KB of JSON; JSON-RPC framing and
 * string escaping add little. Anything larger is refused before it is parsed.
 */
export const MAX_BODY_BYTES = 1024 * 1024;

export interface HostedMcpOptions {
  limits?: Partial<McpLimits>;
  now?: () => number;
  report?: (error: unknown) => void;
}

export interface HostedMcp {
  readonly limits: McpLimits;
  /** Serves one MCP request for an authenticated caller. The request's own credentials are not read again. */
  fetch(request: Request, principal: Principal, auth?: Pick<AuthInfo, 'clientId' | 'expiresAt'>): Promise<Response>;
  close(): Promise<void>;
}

const PRINCIPAL = 'principal';
const LIST_CACHE = { ttlMs: 300_000, cacheScope: 'private' } as const;

export function createHostedMcp(runtime: Runtime, options: HostedMcpOptions = {}): HostedMcp {
  const limits: McpLimits = { ...DEFAULT_LIMITS, ...options.limits };
  const now = options.now ?? Date.now;

  const handler = createMcpHandler(
    ({ authInfo }) => {
      const principal = authInfo?.extra?.[PRINCIPAL] as Principal | undefined;
      // `fetch` below is the only way in, and it always supplies one.
      if (!principal) throw new Error('An MCP request reached the server without a caller.');
      const server = new McpServer(
        { name: SERVER_NAME, title: 'FIRESight', version: SERVER_VERSION, websiteUrl: runtime.config.origin },
        {
          instructions: SERVER_INSTRUCTIONS,
          // The tool list never changes while a client is connected, so there is nothing to subscribe to.
          capabilities: { tools: { listChanged: false } },
          // The same list and the same description for every caller: a client may keep them for five minutes.
          cacheHints: { 'tools/list': LIST_CACHE, 'server/discover': LIST_CACHE },
        },
      );
      registerTools(server, { runtime, principal, limits, now, report: options.report });
      return server;
    },
    {
      // The default response mode answers a 2026-era request with one JSON body unless
      // a handler sends something before its result, and no tool here does. A 2025-era
      // request gets the same single message framed as a server-sent event, after
      // which the response ends. Either way nothing stays open.
      maxRequestBodySize: MAX_BODY_BYTES,
      // No change notifications exist to listen for, and a listening stream would hold
      // a function open for as long as the platform allows.
      maxSubscriptions: 0,
    },
  );

  return {
    limits,
    fetch(request, principal, auth) {
      const authInfo: AuthInfo = {
        // The credential itself stays at the door: tools get who is calling, not the secret.
        token: '',
        clientId: auth?.clientId ?? principal.via,
        scopes: [...principal.scopes],
        expiresAt: auth?.expiresAt,
        resource: new URL(runtime.config.mcpUrl),
        extra: { [PRINCIPAL]: principal },
      };
      return handler.fetch(request, { authInfo });
    },
    close: () => handler.close(),
  };
}
