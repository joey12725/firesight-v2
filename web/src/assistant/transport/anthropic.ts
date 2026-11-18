// Two ways to reach Claude behind one interface.
//
//   byok    The browser calls the Anthropic API directly with the user's own key.
//           The key lives in this browser's localStorage and is sent nowhere else.
//   hosted  The same SDK request goes to this site's /api/chat, which adds the
//           operator's key server-side and streams the response back unchanged.
//
// Both use the official SDK, so request building, SSE parsing and typed errors are
// identical; only the destination and the credential differ. The SDK's own retries
// are off because the agent loop retries visibly and cancellably.
import Anthropic from '@anthropic-ai/sdk';
import type { ModelRequest, ModelStream, Transport } from '../agent/types';
import { buildParams } from './params';

export const HOSTED_PATH = '/api/chat';
export const ACCESS_CODE_HEADER = 'x-firesight-access';

function streamWith(client: Anthropic, request: ModelRequest, signal: AbortSignal): ModelStream {
  return client.beta.messages.stream(buildParams(request), { signal }) as unknown as ModelStream;
}

export function createByokTransport(apiKey: string): Transport {
  // dangerouslyAllowBrowser: the key is the user's own, typed by them into this
  // browser, and the only place it goes is api.anthropic.com.
  const client = new Anthropic({ apiKey, dangerouslyAllowBrowser: true, maxRetries: 0 });
  return { mode: 'byok', stream: (request, signal) => streamWith(client, request, signal) };
}

export interface HostedOptions {
  /** Origin of the app; the proxy lives at `${origin}/api/chat`. */
  origin: string;
  accessCode?: string;
  fetch?: typeof fetch;
}

/** Rewrites the SDK's request to the proxy: same body and version headers, no credential. */
export function hostedFetch(options: HostedOptions): typeof fetch {
  const inner = options.fetch ?? ((input, init) => fetch(input, init));
  return (input, init) => {
    const source = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url);
    const target = new URL(HOSTED_PATH, options.origin);
    if (source.searchParams.get('beta') === 'true') target.searchParams.set('beta', 'true');
    const headers = new Headers(init?.headers);
    headers.delete('x-api-key');
    headers.delete('authorization');
    if (options.accessCode) headers.set(ACCESS_CODE_HEADER, options.accessCode);
    return inner(target.href, { ...init, headers });
  };
}

export function createHostedTransport(options: HostedOptions): Transport {
  const client = new Anthropic({
    // Never sent: hostedFetch strips it. The proxy holds the real key.
    apiKey: 'hosted-proxy',
    baseURL: new URL(HOSTED_PATH, options.origin).href,
    dangerouslyAllowBrowser: true,
    maxRetries: 0,
    fetch: hostedFetch(options),
  });
  return { mode: 'hosted', stream: (request, signal) => streamWith(client, request, signal) };
}
