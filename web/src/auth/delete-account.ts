// Deleting an account: how the person confirms it, and what is sent.
//
// An account that has a password confirms with the password, and the server checks it.
// One that only ever signed in with GitHub or Google has no password to give: any
// password sent for it is refused. It confirms by typing its email address instead, and
// the request goes without a password; the server then accepts it only on a session
// that began less than a day ago.
//
// Kept apart from the page so it can be tested without a browser.

/** A way of signing in that the account has, as `GET /api/auth/list-accounts` gives it. */
export interface SignInMethod {
  providerId?: unknown;
}

export type DeleteConfirmation = 'password' | 'email';

/** The provider id Better Auth gives an email-and-password sign-in. */
const CREDENTIAL = 'credential';

/** Password where the account has one; otherwise the email address, typed out. */
export const confirmationFor = (methods: readonly SignInMethod[]): DeleteConfirmation => (methods.some((m) => m.providerId === CREDENTIAL) ? 'password' : 'email');

/** What has to be typed for the email confirmation. */
export const confirmationPhrase = (email: string): string => email.trim() || 'delete my account';

/** Whether what was typed is enough to send the request. */
export function isConfirmed(mode: DeleteConfirmation, typed: string, email: string): boolean {
  if (mode === 'password') return typed !== '';
  return typed.trim().toLowerCase() === confirmationPhrase(email).toLowerCase();
}

/** The body of the delete request. A password is sent only for an account that has one. */
export const deleteRequest = (mode: DeleteConfirmation, typed: string): { password?: string } => (mode === 'password' ? { password: typed } : {});

export interface DeleteFailure {
  code?: string;
  message?: string;
  status?: number;
}

/** A refusal the page words itself; null leaves it to the general wording for auth errors. */
export function deleteFailureMessage(error: DeleteFailure): string | null {
  if (error.code === 'SESSION_EXPIRED') return 'For your safety, this needs a recent sign-in, and yours is more than a day old. Sign out, sign in again, then delete the account.';
  if (error.code === 'CREDENTIAL_ACCOUNT_NOT_FOUND') return 'This account has no password. Close this, open it again, and confirm with your email address instead.';
  if (error.code === 'INVALID_PASSWORD') return 'That is not the password for this account.';
  return null;
}

export interface DeleteClient {
  listAccounts(): Promise<{ data: readonly SignInMethod[] | null; error: DeleteFailure | null }>;
  deleteUser(body: { password?: string }): Promise<{ error: DeleteFailure | null }>;
}

/** Asks the server how this account signs in. Throws when it does not say. */
export async function readConfirmation(client: Pick<DeleteClient, 'listAccounts'>): Promise<DeleteConfirmation> {
  const result = await client.listAccounts();
  if (result.error || !Array.isArray(result.data)) throw new Error(result.error?.message || 'The server did not answer.');
  return confirmationFor(result.data);
}

/** Sends the delete request. Resolves to null when the account is gone, or to the refusal. */
export async function requestDeletion(client: Pick<DeleteClient, 'deleteUser'>, mode: DeleteConfirmation, typed: string, email: string): Promise<DeleteFailure | null> {
  if (!isConfirmed(mode, typed, email)) return { message: mode === 'password' ? 'Enter your password.' : 'Type your email address exactly to confirm.' };
  return (await client.deleteUser(deleteRequest(mode, typed))).error;
}
