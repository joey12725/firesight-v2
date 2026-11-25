// Deleting an account: an account without a password (GitHub or Google only) is not
// asked for one and none is sent; one with a password is, and it is sent. The form is
// rendered to markup here; the browser journey is e2e/accounts.mjs.
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import { confirmationFor, deleteFailureMessage, isConfirmed, readConfirmation, requestDeletion, type DeleteClient, type DeleteFailure, type SignInMethod } from './delete-account';
import { DeleteAccountForm, type DeleteAccountFormProps } from './pages/DeleteAccount';

const EMAIL = 'Lena@Example.com';
const GITHUB: SignInMethod[] = [{ providerId: 'github' }];
const PASSWORD: SignInMethod[] = [{ providerId: 'credential' }];

function client(methods: SignInMethod[] | null, refusal: DeleteFailure | null = null) {
  const sent: { password?: string }[] = [];
  const api: DeleteClient = {
    listAccounts: async () => (methods ? { data: methods, error: null } : { data: null, error: { status: 500, message: 'boom' } }),
    deleteUser: async (body) => {
      sent.push(body);
      return { error: refusal };
    },
  };
  return { api, sent };
}

const form = (props: Partial<DeleteAccountFormProps>) =>
  renderToStaticMarkup(
    createElement(DeleteAccountForm, { email: EMAIL, mode: 'loading', typed: '', onTyped: vi.fn(), error: null, deleting: false, onCancel: vi.fn(), onSubmit: vi.fn(), onRetry: vi.fn(), ...props }),
  );
const submitDisabled = (markup: string) => /<button[^>]*type="submit"[^>]*disabled=""/.test(markup);

describe('how an account confirms its deletion', () => {
  it('is by password only when the account has one', async () => {
    expect(confirmationFor(PASSWORD)).toBe('password');
    expect(confirmationFor([...GITHUB, ...PASSWORD])).toBe('password');
    expect(confirmationFor(GITHUB)).toBe('email');
    expect(confirmationFor([{ providerId: 'google' }, { providerId: 'github' }])).toBe('email');
    expect(confirmationFor([])).toBe('email');
    expect(await readConfirmation(client(GITHUB).api)).toBe('email');
    expect(await readConfirmation(client(PASSWORD).api)).toBe('password');
  });

  it('is not guessed when the server does not say how the account signs in', async () => {
    await expect(readConfirmation(client(null).api)).rejects.toThrow('boom');
  });

  it('by email needs the address itself, whatever its case', () => {
    expect(isConfirmed('email', 'lena@example.com ', EMAIL)).toBe(true);
    expect(isConfirmed('email', 'lena@example.co', EMAIL)).toBe(false);
    expect(isConfirmed('email', '', EMAIL)).toBe(false);
    expect(isConfirmed('password', '', EMAIL)).toBe(false);
    expect(isConfirmed('password', 'x', EMAIL)).toBe(true);
  });
});

describe('the delete request', () => {
  it('carries no password for an account that has none', async () => {
    const t = client(GITHUB);
    expect(await requestDeletion(t.api, 'email', 'lena@example.com', EMAIL)).toBeNull();
    expect(t.sent).toEqual([{}]);
    expect('password' in (t.sent[0] as object)).toBe(false);
  });

  it('carries the password for an account that has one', async () => {
    const t = client(PASSWORD);
    expect(await requestDeletion(t.api, 'password', 'correct horse battery', EMAIL)).toBeNull();
    expect(t.sent).toEqual([{ password: 'correct horse battery' }]);
  });

  it('is not sent until the confirmation is complete', async () => {
    const t = client(GITHUB);
    expect(await requestDeletion(t.api, 'email', 'someone@else.example', EMAIL)).toMatchObject({ message: expect.stringContaining('email address') });
    expect(await requestDeletion(t.api, 'password', '', EMAIL)).toMatchObject({ message: expect.stringContaining('password') });
    expect(t.sent).toEqual([]);
  });

  it('hands back the server’s refusal, and words an old session plainly', async () => {
    const expired = { code: 'SESSION_EXPIRED', status: 400, message: 'Session expired. Re-authenticate to perform this action.' };
    const t = client(GITHUB, expired);
    const failure = await requestDeletion(t.api, 'email', EMAIL, EMAIL);
    expect(failure).toEqual(expired);
    expect(deleteFailureMessage(failure as DeleteFailure)).toMatch(/sign in again, then delete the account/i);
    expect(deleteFailureMessage({ code: 'INVALID_PASSWORD' })).toMatch(/not the password/);
    expect(deleteFailureMessage({ code: 'CREDENTIAL_ACCOUNT_NOT_FOUND' })).toMatch(/no password/);
    expect(deleteFailureMessage({ status: 429 })).toBeNull();
  });
});

describe('the delete form', () => {
  it('shows no password field to an account without a password, and asks for the email instead', () => {
    const markup = form({ mode: 'email' });
    expect(markup).not.toContain('type="password"');
    expect(markup).not.toContain('Your password, to confirm');
    expect(markup).toContain('Type your email address to confirm');
    expect(markup).toContain(EMAIL);
    expect(markup).toContain('data-confirm="email"');
    expect(submitDisabled(markup)).toBe(true);
    expect(submitDisabled(form({ mode: 'email', typed: 'lena@example.com' }))).toBe(false);
  });

  it('asks an account with a password for it', () => {
    const markup = form({ mode: 'password' });
    expect(markup).toContain('type="password"');
    expect(markup).toContain('Your password, to confirm');
    expect(markup).not.toContain('Type your email address to confirm');
    expect(submitDisabled(markup)).toBe(true);
    expect(submitDisabled(form({ mode: 'password', typed: 'x' }))).toBe(false);
  });

  it('cannot be submitted before it is known how the account signs in', () => {
    for (const mode of ['loading', 'unknown'] as const) {
      const markup = form({ mode, typed: 'anything' });
      expect(markup).not.toContain('<input');
      expect(submitDisabled(markup)).toBe(true);
    }
    expect(form({ mode: 'unknown' })).toContain('Try again');
  });

  it('shows the refusal', () => {
    expect(form({ mode: 'email', error: 'Sign out, sign in again, then delete the account.' })).toContain('role="alert"');
  });
});
