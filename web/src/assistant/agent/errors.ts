// Turns whatever a request threw into something a person can act on. Classification
// uses the SDK's typed error classes and the structured error body, never message text.
import Anthropic from '@anthropic-ai/sdk';
import { isAbortSignalError } from '../tools/abort';
import type { AssistantError, TransportMode } from './types';

/** `error.type` values the hosted proxy (api/chat.ts) uses in its Anthropic-shaped error bodies. */
export const PROXY_ERRORS = {
  accessCode: 'firesight_access_code',
  rateLimit: 'firesight_rate_limit',
  origin: 'firesight_origin',
  unavailable: 'firesight_unavailable',
  rejected: 'firesight_rejected',
  tooLarge: 'firesight_too_large',
} as const;

export function isAbortError(error: unknown): boolean {
  if (error instanceof Anthropic.APIUserAbortError) return true;
  return isAbortSignalError(error);
}

function bodyType(error: InstanceType<typeof Anthropic.APIError>): string | undefined {
  const body = error.error as { error?: { type?: unknown } } | undefined;
  return typeof body?.error?.type === 'string' ? body.error.type : undefined;
}

function bodyMessage(error: InstanceType<typeof Anthropic.APIError>): string | undefined {
  const body = error.error as { error?: { message?: unknown } } | undefined;
  return typeof body?.error?.message === 'string' ? body.error.message : undefined;
}

function retryAfterMs(error: InstanceType<typeof Anthropic.APIError>): number | undefined {
  const raw = error.headers?.get('retry-after');
  const seconds = raw ? Number(raw) : NaN;
  return Number.isFinite(seconds) && seconds >= 0 ? seconds * 1000 : undefined;
}

export function classifyError(error: unknown, mode: TransportMode): AssistantError {
  const hosted = mode === 'hosted';
  if (error instanceof Anthropic.APIError) {
    const detail = bodyMessage(error);
    const type = bodyType(error);
    if (type === PROXY_ERRORS.accessCode) {
      return { kind: 'access_code', title: 'Access code needed', message: 'This site asks for an access code before using its hosted assistant. Enter it in the assistant options.', retryable: false };
    }
    if (type === PROXY_ERRORS.unavailable) {
      return { kind: 'unavailable', title: 'Hosted assistant is off', message: 'The site operator has not configured the hosted assistant. Add your own Claude API key in Settings to use it.', retryable: false };
    }
    if (type === PROXY_ERRORS.origin || type === PROXY_ERRORS.rejected) {
      return { kind: 'bad_request', title: 'The hosted assistant refused the request', message: 'The proxy only accepts requests from this app in its current version. Reload the page and try again.', detail, retryable: false };
    }
    if (type === PROXY_ERRORS.tooLarge || error instanceof Anthropic.ContentTooLargeError) {
      return {
        kind: 'too_large',
        title: 'Too much to send',
        message: hosted
          ? 'The hosted assistant accepts about 4 MB per request, attachments included. Remove or shrink the attachments, clear the conversation, or use your own API key (32 MB).'
          : 'The request is over the 32 MB limit. Remove or shrink the attachments, or clear the conversation.',
        detail,
        retryable: false,
      };
    }
    if (error instanceof Anthropic.AuthenticationError) {
      return hosted
        ? { kind: 'auth', title: 'The hosted assistant is misconfigured', message: "The site's API key was rejected. The operator needs to check ANTHROPIC_API_KEY.", detail, retryable: false }
        : { kind: 'auth', title: 'The API key was not accepted', message: 'Check the Claude API key in Settings: it may be mistyped, revoked, or from another workspace.', detail, retryable: false };
    }
    if (error instanceof Anthropic.PermissionDeniedError) {
      return { kind: 'permission', title: 'This key cannot use that model', message: 'The API key does not have access to the selected model. Pick another model in Settings.', detail, retryable: false };
    }
    if (error instanceof Anthropic.NotFoundError) {
      return { kind: 'not_found', title: 'Model not available', message: 'The selected model was not found for this key. Pick another model in Settings.', detail, retryable: false };
    }
    if (error instanceof Anthropic.RateLimitError) {
      const wait = retryAfterMs(error);
      return {
        kind: 'rate_limit',
        title: 'Rate limit reached',
        message:
          type === PROXY_ERRORS.rateLimit
            ? 'The hosted assistant limits how many requests one address can send. Wait a minute and try again, or use your own API key.'
            : wait === undefined
              ? "The API account's rate or spend limit was reached. If it does not clear in a minute, check the limits in the Anthropic console."
              : "The API account's rate limit was reached. Trying again shortly usually works.",
        detail,
        retryable: wait !== undefined || type === 'rate_limit_error',
        retryAfterMs: wait,
      };
    }
    if (error instanceof Anthropic.BadRequestError) {
      return {
        kind: 'bad_request',
        title: 'The API rejected the request',
        message: 'This is usually a model the account cannot use as configured (some models need 30-day data retention enabled), or an attachment the API could not read.',
        detail,
        retryable: false,
      };
    }
    if (error instanceof Anthropic.APIConnectionError) {
      return { kind: 'network', title: 'Could not reach Claude', message: 'The connection failed or timed out. Check your network and try again.', retryable: true };
    }
    if (error instanceof Anthropic.InternalServerError) {
      return error.status === 529
        ? { kind: 'overloaded', title: 'Claude is overloaded', message: 'The API is at capacity right now. Try again in a moment, or switch to another model in Settings.', detail, retryable: true }
        : { kind: 'server', title: 'The API had a problem', message: 'Claude returned a server error. Try again in a moment.', detail, retryable: true };
    }
    return { kind: 'unknown', title: 'The request failed', message: `The API returned an error${error.status ? ` (${error.status})` : ''}.`, detail, retryable: false };
  }
  if (error instanceof TypeError) {
    // What fetch throws when the network is down or the request is blocked.
    return { kind: 'network', title: 'Could not reach Claude', message: 'The connection failed. Check your network and try again.', retryable: true };
  }
  return {
    kind: 'unknown',
    title: 'Something went wrong',
    message: 'The response could not be read. Try again.',
    detail: error instanceof Error ? error.message : undefined,
    // A stream that broke mid-way or a tool input that would not parse: worth one more go.
    retryable: true,
  };
}
