// Envelope encryption for synced plans. node:crypto only.
//
//   PLAN_KEK (server key, in the environment)
//     └─ wraps one random 256-bit data key per user        (table user_key)
//          └─ encrypts each of that user's plan documents  (table plan)
//
// Everything is AES-256-GCM with a fresh 96-bit nonce. Additional authenticated data
// ties each ciphertext to where it belongs: a data key to its user, a document to its
// user and plan. A row copied onto another user or another plan fails to decrypt
// instead of quietly showing someone else's numbers.
//
// What this is and is not. It protects against a database dump, a backup, a leaked
// connection string and the database provider's console: all of those see only
// ciphertext. It does not protect against this server, which holds the key and must
// read plans to validate and simulate them for the agents a user connects. It is
// encryption at rest with a server-held key, not end-to-end encryption.
//
// Deleting a user deletes their user_key row (foreign key, on delete cascade), after
// which any copy of their ciphertext, in a backup say, can no longer be read.

import { createCipheriv, createDecipheriv, randomBytes } from 'node:crypto';

const VERSION = 1;
const IV_BYTES = 12;
const TAG_BYTES = 16;
const HEADER_BYTES = 1 + IV_BYTES + TAG_BYTES;
export const KEY_BYTES = 32;

/** Raised when a ciphertext does not authenticate: wrong key, wrong owner, or altered bytes. */
export class DecryptError extends Error {
  constructor() {
    // Deliberately uninformative: the cause is never something to show a caller.
    super('The stored data could not be decrypted.');
  }
}

/** Seals `plaintext` as `version(1) | iv(12) | tag(16) | ciphertext`. */
export function seal(key: Buffer, plaintext: Buffer, aad: string): Buffer {
  if (key.length !== KEY_BYTES) throw new TypeError('The key must be 32 bytes.');
  const iv = randomBytes(IV_BYTES);
  const cipher = createCipheriv('aes-256-gcm', key, iv);
  cipher.setAAD(Buffer.from(aad, 'utf8'));
  const body = Buffer.concat([cipher.update(plaintext), cipher.final()]);
  return Buffer.concat([Buffer.from([VERSION]), iv, cipher.getAuthTag(), body]);
}

export function open(key: Buffer, sealed: Uint8Array, aad: string): Buffer {
  if (key.length !== KEY_BYTES) throw new TypeError('The key must be 32 bytes.');
  const bytes = Buffer.from(sealed.buffer, sealed.byteOffset, sealed.byteLength);
  if (bytes.length < HEADER_BYTES || bytes[0] !== VERSION) throw new DecryptError();
  const decipher = createDecipheriv('aes-256-gcm', key, bytes.subarray(1, 1 + IV_BYTES), { authTagLength: TAG_BYTES });
  decipher.setAAD(Buffer.from(aad, 'utf8'));
  decipher.setAuthTag(bytes.subarray(1 + IV_BYTES, HEADER_BYTES));
  try {
    return Buffer.concat([decipher.update(bytes.subarray(HEADER_BYTES)), decipher.final()]);
  } catch {
    throw new DecryptError();
  }
}

export const newDataKey = (): Buffer => randomBytes(KEY_BYTES);

// The separators cannot occur in a user id or a plan id, so the AAD strings are unambiguous.
const keyAad = (userId: string) => `firesight:user-key|${userId}`;

/** `user_id|plan_id`, with the field named so a name can never stand in for a document. */
export const planAad = (userId: string, planId: string, field: 'doc' | 'name') => `firesight:plan-${field}|${userId}|${planId}`;

export function wrapDataKey(kek: Buffer, dataKey: Buffer, userId: string): Buffer {
  return seal(kek, dataKey, keyAad(userId));
}

export function unwrapDataKey(kek: Buffer, wrapped: Uint8Array, userId: string): Buffer {
  const key = open(kek, wrapped, keyAad(userId));
  if (key.length !== KEY_BYTES) throw new DecryptError();
  return key;
}
