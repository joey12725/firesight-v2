// The form inside the "Delete your account?" dialog. What it asks for depends on how the
// account signs in (auth/delete-account.ts): a password where there is one, the email
// address typed out where there is not.
import type { FormEvent } from 'react';
import { Button, Field, Skeleton, TextInput } from '@/ui';
import { confirmationPhrase, isConfirmed, type DeleteConfirmation } from '../delete-account';
import { ErrorNote } from './shared';

export interface DeleteAccountFormProps {
  email: string;
  /** `loading` while the server is asked how the account signs in; `unknown` when it did not say. */
  mode: DeleteConfirmation | 'loading' | 'unknown';
  typed: string;
  onTyped: (value: string) => void;
  error: string | null;
  deleting: boolean;
  onCancel: () => void;
  onSubmit: () => void;
  onRetry: () => void;
}

export function DeleteAccountForm({ email, mode, typed, onTyped, error, deleting, onCancel, onSubmit, onRetry }: DeleteAccountFormProps) {
  const ready = mode === 'password' || mode === 'email';
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (ready && !deleting) onSubmit();
  };
  return (
    <form onSubmit={submit} className="space-y-4" data-testid="delete-account" data-confirm={mode}>
      <ul className="space-y-1.5 text-sm leading-relaxed text-ink-2">
        <li>Every plan in the account is deleted from the server, with the key that encrypted them.</li>
        <li>Your API keys stop working and every connected app is disconnected.</li>
        <li>Plans you have opened in this browser stay in this browser, unsynced. Export anything else first.</li>
      </ul>
      {mode === 'loading' && <Skeleton className="h-10 w-full" />}
      {mode === 'unknown' && (
        <p className="text-sm leading-relaxed text-ink-2">
          The server did not say how this account signs in, so it cannot be confirmed yet.{' '}
          <button type="button" className="font-medium text-accent-ink hover:underline" onClick={onRetry}>
            Try again
          </button>
        </p>
      )}
      {mode === 'password' && (
        <Field label="Your password, to confirm">{({ id }) => <TextInput id={id} type="password" autoComplete="current-password" value={typed} onChange={onTyped} />}</Field>
      )}
      {mode === 'email' && (
        <Field
          label="Type your email address to confirm"
          hint={
            <>
              This account signs in with GitHub or Google and has no password, so type <span className="num break-all text-ink-1">{confirmationPhrase(email)}</span> instead. If you signed in more than a day ago, sign out and
              sign in again first.
            </>
          }
        >
          {({ id, describedBy }) => <TextInput id={id} aria-describedby={describedBy} type="text" autoCapitalize="none" placeholder={confirmationPhrase(email)} value={typed} onChange={onTyped} />}
        </Field>
      )}
      <ErrorNote>{error}</ErrorNote>
      <div className="flex justify-end gap-2">
        <Button disabled={deleting} onClick={onCancel}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" className="bg-bad hover:bg-bad" disabled={deleting || !ready || !isConfirmed(mode, typed, email)}>
          {deleting ? 'Deleting…' : 'Delete my account'}
        </Button>
      </div>
    </form>
  );
}
