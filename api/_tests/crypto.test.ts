import { randomBytes } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import { DecryptError, newDataKey, open, planAad, seal, unwrapDataKey, wrapDataKey } from '../_lib/crypto.js';

const key = () => randomBytes(32);

describe('seal and open', () => {
  it('round-trips, including empty and large plaintexts', () => {
    const k = key();
    for (const text of ['', 'x', JSON.stringify({ salary: 120000, note: 'héllo 🌍' }), 'a'.repeat(300_000)]) {
      const sealed = seal(k, Buffer.from(text, 'utf8'), 'aad');
      expect(open(k, sealed, 'aad').toString('utf8')).toBe(text);
    }
  });

  it('uses a fresh nonce every time, so equal plaintexts do not produce equal ciphertexts', () => {
    const k = key();
    const a = seal(k, Buffer.from('same'), 'aad');
    const b = seal(k, Buffer.from('same'), 'aad');
    expect(a.equals(b)).toBe(false);
    expect(a.subarray(1, 13).equals(b.subarray(1, 13))).toBe(false);
  });

  it('does not contain the plaintext', () => {
    const sealed = seal(key(), Buffer.from('salary 120000 at Initech'), 'aad');
    expect(sealed.includes('Initech')).toBe(false);
    expect(sealed.includes('120000')).toBe(false);
  });

  it('rejects a flipped bit anywhere: version, nonce, tag or body', () => {
    const k = key();
    const sealed = seal(k, Buffer.from('the plan'), 'aad');
    for (let i = 0; i < sealed.length; i++) {
      const copy = Buffer.from(sealed);
      copy[i] = (copy[i] ?? 0) ^ 0x01;
      expect(() => open(k, copy, 'aad'), `byte ${i}`).toThrow(DecryptError);
    }
  });

  it('rejects the wrong key, the wrong additional data, and truncated input', () => {
    const k = key();
    const sealed = seal(k, Buffer.from('the plan'), 'user-a|plan-1');
    expect(() => open(key(), sealed, 'user-a|plan-1')).toThrow(DecryptError);
    expect(() => open(k, sealed, 'user-b|plan-1')).toThrow(DecryptError);
    expect(() => open(k, sealed, 'user-a|plan-2')).toThrow(DecryptError);
    expect(() => open(k, sealed.subarray(0, 20), 'user-a|plan-1')).toThrow(DecryptError);
    expect(() => open(k, sealed.subarray(0, sealed.length - 1), 'user-a|plan-1')).toThrow(DecryptError);
    expect(() => open(k, Buffer.alloc(0), 'user-a|plan-1')).toThrow(DecryptError);
  });

  it('says nothing about why a decrypt failed', () => {
    const sealed = seal(key(), Buffer.from('x'), 'aad');
    try {
      open(key(), sealed, 'aad');
      expect.unreachable();
    } catch (error) {
      expect((error as Error).message).toBe('The stored data could not be decrypted.');
    }
  });

  it('insists on 32-byte keys', () => {
    expect(() => seal(randomBytes(16), Buffer.from('x'), 'aad')).toThrow(TypeError);
    expect(() => open(randomBytes(31), Buffer.alloc(40), 'aad')).toThrow(TypeError);
  });
});

describe('data keys', () => {
  it('wraps and unwraps a data key for its user only', () => {
    const kek = key();
    const dek = newDataKey();
    const wrapped = wrapDataKey(kek, dek, 'user-a');
    expect(unwrapDataKey(kek, wrapped, 'user-a').equals(dek)).toBe(true);
    expect(() => unwrapDataKey(kek, wrapped, 'user-b')).toThrow(DecryptError);
    expect(() => unwrapDataKey(key(), wrapped, 'user-a')).toThrow(DecryptError);
  });

  it('binds a document to its user, its plan and its field', () => {
    expect(planAad('u1', 'p1', 'doc')).not.toBe(planAad('u1', 'p1', 'name'));
    expect(planAad('u1', 'p1', 'doc')).not.toBe(planAad('u1', 'p2', 'doc'));
    expect(planAad('u1', 'p1', 'doc')).not.toBe(planAad('u2', 'p1', 'doc'));
    expect(planAad('u1', 'p1', 'doc')).toContain('u1|p1');
  });
});
