// Test support for the MCP endpoint: real MCP clients (the SDK's own) talking to the
// real function in process, with real credentials from the auth API.

import { Client, StreamableHTTPClientTransport } from '@modelcontextprotocol/client';
import { handle as handleMcp } from '../mcp.js';
import type { Deps } from '../mcp.js';
import { Browser, ORIGIN } from './harness.js';
import type { TestRuntime } from './harness.js';

export const MCP_URL = `${ORIGIN}/api/mcp`;

/** Calls the function as the platform would. A failure that is the server's fault fails the test. */
export function callMcp(t: TestRuntime, request: Request, deps: Deps = {}): Promise<Response> {
  return handleMcp(request, {}, {
    state: t.state,
    report: (error) => {
      throw error;
    },
    ...deps,
  });
}

let users = 0;

/** A fresh account with an API key of the given kind. */
export async function userWithKey(t: TestRuntime, kind: 'read' | 'write' = 'write', name = 'Test key'): Promise<{ browser: Browser; userId: string; key: string }> {
  const browser = new Browser(t.state);
  const userId = await browser.signUp(`mcp-${users++}-${Date.now()}@example.com`);
  return { browser, userId, key: await createKey(browser, kind, name) };
}

export async function createKey(browser: Browser, kind: 'read' | 'write', name = 'Test key'): Promise<string> {
  const created = await browser.post<{ key: string }>('/api/auth/api-key/create', { name, ...(kind === 'write' ? { configId: 'read-write' } : {}) });
  if (created.status !== 200) throw new Error(`Key creation failed: ${created.status} ${created.text}`);
  return created.body.key;
}

export interface Connected {
  client: Client;
  /** Every HTTP exchange the client made: method, the JSON-RPC method when there was one, and the status. */
  exchanges: { method: string; rpc: string | null; status: number }[];
  close(): Promise<void>;
}

/**
 * Connects an MCP client with a bearer token. `era` picks the protocol: 'modern' pins
 * the 2026-07-28 revision, 'legacy' is a client from before it (the `initialize`
 * handshake), 'auto' probes as a current client would.
 */
export async function connect(t: TestRuntime, token: string, options: { era?: 'modern' | 'legacy' | 'auto'; deps?: Deps } = {}): Promise<Connected> {
  const exchanges: Connected['exchanges'] = [];
  const transport = new StreamableHTTPClientTransport(new URL(MCP_URL), {
    requestInit: { headers: { authorization: `Bearer ${token}` } },
    fetch: async (url, init) => {
      const request = new Request(url, init);
      let rpc: string | null = null;
      try {
        rpc = ((await request.clone().json()) as { method?: string }).method ?? null;
      } catch {
        // No JSON body (a GET or DELETE).
      }
      const response = await callMcp(t, request, options.deps);
      exchanges.push({ method: request.method, rpc, status: response.status });
      return response;
    },
  });
  const era = options.era ?? 'modern';
  const client = new Client({ name: 'firesight-tests', version: '1.0.0' }, { versionNegotiation: { mode: era === 'modern' ? { pin: '2026-07-28' } : era } });
  await client.connect(transport);
  return { client, exchanges, close: () => client.close() };
}

export interface ToolReply<T = Record<string, unknown>> {
  isError: boolean;
  data: T;
  /** The text blocks, in order. */
  text: string[];
}

export async function callTool<T = Record<string, unknown>>(client: Client, name: string, args: Record<string, unknown> = {}): Promise<ToolReply<T>> {
  const result = await client.callTool({ name, arguments: args });
  const content = (result.content ?? []) as { type: string; text?: string }[];
  return { isError: result.isError === true, data: (result.structuredContent ?? {}) as T, text: content.filter((c) => c.type === 'text').map((c) => c.text ?? '') };
}

/** A household the engine can simulate, small enough to read in a test. */
export function sampleProfile(name = 'Lena') {
  return {
    name: `${name}'s plan`,
    household: { people: [{ id: 'lena', name, birth: '1990-04' }], location: { country: 'US', state: 'TX', city: 'Austin' } },
    incomes: [{ id: 'lena_job', name: 'Product designer', owner: 0, kind: { type: 'job', salary: 128_000, plan: { employee_fraction: 0.08, match_rate: 0.5, match_up_to: 0.06 } } }],
    spending: { items: [{ id: 'living', name: 'Living costs', category: 'other', amount: 4_500, frequency: 'monthly' }] },
    accounts: [{ id: 'lena_401k', name: '401(k)', owner: 0, kind: 'traditional', balance: 96_000 }],
    simulation: { paths: 400, seed: 11 },
  };
}
