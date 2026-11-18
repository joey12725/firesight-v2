// Share links: the plan is compressed into the URL fragment. Fragments are never sent
// to a server, so the profile stays between the two browsers.
import type { Profile } from '@/engine/types';
import type { RunSettings, Scenario } from './profile-store';

export interface SharePayload {
  v: 2;
  profile: Profile;
  scenarios: Scenario[];
  run: RunSettings;
}

const PARAM = 'plan';

function toBase64Url(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function fromBase64Url(text: string): Uint8Array<ArrayBuffer> {
  const binary = atob(text.replace(/-/g, '+').replace(/_/g, '/'));
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

async function pipe(bytes: Uint8Array<ArrayBuffer>, transform: TransformStream<BufferSource, Uint8Array>) {
  const stream = new Blob([bytes]).stream().pipeThrough(transform);
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

export async function encodeShare(payload: SharePayload): Promise<string> {
  const json = new TextEncoder().encode(JSON.stringify(payload));
  return toBase64Url(await pipe(json, new CompressionStream('deflate-raw')));
}

export async function decodeShare(encoded: string): Promise<SharePayload> {
  const bytes = await pipe(fromBase64Url(encoded), new DecompressionStream('deflate-raw'));
  const payload = JSON.parse(new TextDecoder().decode(bytes)) as SharePayload;
  if (payload?.v !== 2 || typeof payload.profile !== 'object' || payload.profile === null) {
    throw new Error('This link does not contain a FIRESight plan.');
  }
  return { ...payload, scenarios: Array.isArray(payload.scenarios) ? payload.scenarios : [] };
}

export async function shareUrl(payload: SharePayload, base = window.location.href): Promise<string> {
  const url = new URL(base);
  url.hash = `${PARAM}=${await encodeShare(payload)}`;
  return url.toString();
}

/** The encoded plan in a URL fragment, if there is one. */
export function shareParam(hash = window.location.hash): string | null {
  return new URLSearchParams(hash.replace(/^#/, '')).get(PARAM);
}
