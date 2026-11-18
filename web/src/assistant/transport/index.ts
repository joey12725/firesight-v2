// Chooses the transport: the user's own key when there is one, else the site's hosted
// proxy when the operator has configured it, else nothing (the rest of the app works
// without the assistant).
import type { AssistantModel } from '@/state/settings-store';
import { isAssistantModel } from '../models';
import type { Transport } from '../agent/types';
import { createByokTransport, createHostedTransport, HOSTED_PATH } from './anthropic';

export * from './anthropic';
export * from './params';

export interface HostedInfo {
  available: boolean;
  /** Models the operator allows, most capable first. */
  models: AssistantModel[];
  accessCodeRequired: boolean;
}

export const HOSTED_OFF: HostedInfo = { available: false, models: [], accessCodeRequired: false };

let detecting: Promise<HostedInfo> | null = null;

/** Asks GET /api/chat whether the hosted proxy is configured. Cached for the session. */
export function detectHosted(fetchImpl: typeof fetch = fetch, origin = globalThis.location?.origin): Promise<HostedInfo> {
  if (!origin) return Promise.resolve(HOSTED_OFF);
  detecting ??= fetchImpl(new URL(HOSTED_PATH, origin).href, { headers: { accept: 'application/json' }, cache: 'no-store' })
    .then(async (response): Promise<HostedInfo> => {
      // Under `vite dev` and on static hosts the path serves the app shell, not JSON.
      if (!response.ok || !(response.headers.get('content-type') ?? '').includes('application/json')) return HOSTED_OFF;
      const body = (await response.json()) as { available?: unknown; models?: unknown; access_code_required?: unknown };
      const models = Array.isArray(body.models) ? body.models.filter((m): m is AssistantModel => typeof m === 'string' && isAssistantModel(m)) : [];
      return { available: body.available === true && models.length > 0, models, accessCodeRequired: body.access_code_required === true };
    })
    .catch(() => HOSTED_OFF);
  return detecting;
}

export function resetHostedDetection() {
  detecting = null;
}

export interface TransportChoice {
  transport: Transport;
  model: AssistantModel;
}

export interface TransportSettings {
  apiKey: string;
  model: AssistantModel;
  accessCode: string;
}

/** Bring-your-own-key wins when a key is present; otherwise the hosted proxy if there is one. */
export function chooseTransport(settings: TransportSettings, hosted: HostedInfo, origin = globalThis.location?.origin ?? ''): TransportChoice | null {
  if (settings.apiKey) return { transport: createByokTransport(settings.apiKey), model: settings.model };
  if (hosted.available && origin) {
    const model = hosted.models.includes(settings.model) ? settings.model : (hosted.models[0] as AssistantModel);
    return { transport: createHostedTransport({ origin, accessCode: settings.accessCode || undefined }), model };
  }
  return null;
}
