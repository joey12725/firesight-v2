import { useEffect, useState, type FormEvent } from 'react';
import { navigate, safeNext } from '@/state/route';
import { Button, Field, Icon, SegmentedControl, TextInput } from '@/ui';
import { refreshSession, signOut, useAccount } from '../account';
import { authClient, authMessage } from '../client';
import { ErrorNote, PageFrame } from './shared';

const MIN_PASSWORD = 12;
const PROVIDER_NAMES: Record<string, string> = { github: 'GitHub', google: 'Google' };

type Mode = 'sign-in' | 'sign-up';
type Redirect = { redirect?: boolean; url?: string } | null;

/**
 * Sign in or create an account. Reached from the top bar, from a plan link opened while
 * signed out (`?next=`), and from an AI tool asking to connect: the server then sends
 * the browser here with a signed query, and signing in continues that request.
 */
export function Login() {
  const status = useAccount((s) => s.status);
  const user = useAccount((s) => s.user);
  const social = useAccount((s) => s.config?.social ?? []);
  const [mode, setMode] = useState<Mode>('sign-in');
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const params = new URLSearchParams(window.location.search);
  // A request from an app in progress: the server signed the query, and takes it back with the sign-in.
  const connecting = params.has('sig');
  const next = safeNext(params.get('next'), window.location.origin);

  // Already signed in and nothing waiting: there is nothing to do on this page.
  useEffect(() => {
    if (status === 'signed-in' && !connecting && !busy) navigate(next, { replace: true });
  }, [status, connecting, busy, next]);

  const leave = (data: Redirect) => {
    // With an app waiting, the server answers with where to go next (the consent page, or straight back
    // to the app) and the auth client is already taking the browser there.
    if (data?.redirect && data.url) return;
    void refreshSession().then(() => navigate(next, { replace: true }));
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setError(null);
    if (mode === 'sign-up' && password.length < MIN_PASSWORD) {
      setError(`Choose a password of at least ${MIN_PASSWORD} characters.`);
      return;
    }
    setBusy(true);
    const result = mode === 'sign-in' ? await authClient.signIn.email({ email: email.trim(), password }) : await authClient.signUp.email({ email: email.trim(), password, name: name.trim() || email.trim().split('@')[0] || 'Me' });
    if (result.error) {
      setBusy(false);
      setError(authMessage(result.error, mode === 'sign-in' ? 'That email and password do not match an account.' : 'The account could not be created.'));
      return;
    }
    leave(result.data as Redirect);
  };

  const carryOn = async () => {
    setBusy(true);
    setError(null);
    const result = await authClient.oauth2.continue({ selected: true });
    // On success the auth client follows the address in the answer; this page is on its way out.
    if (result.error || !result.data?.url) {
      setBusy(false);
      setError(authMessage(result.error, 'The request from the app could not be continued. Start again from the app.'));
    }
  };

  const withProvider = async (provider: string) => {
    setBusy(true);
    setError(null);
    const result = await authClient.signIn.social({ provider, callbackURL: next });
    if (result.error) {
      setBusy(false);
      setError(authMessage(result.error, `Signing in with ${PROVIDER_NAMES[provider] ?? provider} did not start.`));
    }
  };

  if (status === 'signed-in' && connecting) {
    return (
      <PageFrame>
        <h1 className="text-2xl font-semibold tracking-tight">An app is asking to connect</h1>
        <p className="mt-2 text-sm leading-relaxed text-ink-2">
          You are signed in as <span className="font-medium text-ink-1">{user?.email}</span>. Continue to see which app it is and what it is asking for. Nothing is shared until you allow it.
        </p>
        <div className="mt-5 flex flex-wrap gap-2">
          <Button variant="primary" disabled={busy} onClick={() => void carryOn()}>
            Continue as {user?.name || user?.email}
          </Button>
          <Button disabled={busy} onClick={() => void signOut()}>
            Use another account
          </Button>
        </div>
        <div className="mt-4">
          <ErrorNote>{error}</ErrorNote>
        </div>
      </PageFrame>
    );
  }

  return (
    <PageFrame>
      <h1 className="text-2xl font-semibold tracking-tight">{connecting ? 'Sign in to connect an app' : mode === 'sign-in' ? 'Sign in' : 'Create an account'}</h1>
      <p className="mt-2 text-sm leading-relaxed text-ink-2">
        {connecting
          ? 'An AI tool is asking to work with your FIRESight plans. Sign in first; the next page shows which app it is and what it wants, and nothing is shared until you allow it.'
          : 'An account is optional. It lets you keep chosen plans on this site, open them on another device, and let AI tools you connect read and edit them.'}
      </p>

      <SegmentedControl<Mode>
        aria-label="Sign in or create an account"
        className="mt-6"
        value={mode}
        onChange={(m) => {
          setMode(m);
          setError(null);
        }}
        options={[
          { value: 'sign-in', label: 'Sign in' },
          { value: 'sign-up', label: 'Create account' },
        ]}
      />

      <form onSubmit={(e) => void submit(e)} className="mt-5 space-y-4" noValidate>
        {mode === 'sign-up' && (
          <Field label="Name">{({ id }) => <TextInput id={id} name="name" autoComplete="name" value={name} onChange={setName} placeholder="What to call you" />}</Field>
        )}
        <Field label="Email">{({ id }) => <TextInput id={id} name="email" type="email" autoComplete="email" inputMode="email" required value={email} onChange={setEmail} />}</Field>
        <Field label="Password" hint={mode === 'sign-up' ? `At least ${MIN_PASSWORD} characters. There is no password reset yet (this site sends no email), so keep it somewhere safe.` : undefined}>
          {({ id, describedBy }) => (
            <TextInput id={id} aria-describedby={describedBy} name="password" type="password" autoComplete={mode === 'sign-up' ? 'new-password' : 'current-password'} required value={password} onChange={setPassword} />
          )}
        </Field>
        <ErrorNote>{error}</ErrorNote>
        <Button type="submit" variant="primary" size="lg" className="w-full" disabled={busy || email.trim() === '' || password === ''}>
          {busy ? 'One moment…' : mode === 'sign-in' ? 'Sign in' : 'Create account'}
        </Button>
      </form>

      {social.length > 0 && (
        <div className="mt-4 space-y-2 border-t border-line pt-4">
          {social.map((provider) => (
            <Button key={provider} size="lg" className="w-full" disabled={busy} onClick={() => void withProvider(provider)}>
              Continue with {PROVIDER_NAMES[provider] ?? provider}
            </Button>
          ))}
        </div>
      )}

      <div className="mt-8 border-t border-line pt-4 text-xs leading-relaxed text-ink-3">
        <p className="flex items-center gap-1.5 font-medium text-ink-2">
          <Icon name="lock" size={12} /> What an account changes
        </p>
        <p className="mt-1">
          Without an account, your plans stay in this browser and are sent nowhere. With one, only the plans you turn on "Sync to account" for, and plans an AI tool you authorised writes for you, are stored on this
          site's server. They are encrypted where they are stored, with a key the server holds, so the server can read them: it has to, to run simulations for your AI tools. You can export or delete everything
          from the account page.
        </p>
      </div>
    </PageFrame>
  );
}
