import { useEffect, useState } from 'react';
import { navigate } from '@/state/route';
import { Button, Icon, Skeleton } from '@/ui';
import { useAccount } from '../account';
import { authClient, authMessage } from '../client';
import { ErrorNote, PageFrame, scopeMeanings } from './shared';

/** The host of a URL, or null when the text is not one. */
function hostOf(value: string): string | null {
  try {
    return new URL(value).host || null;
  } catch {
    return null;
  }
}

const isLoopback = (host: string) => /^(localhost|127\.0\.0\.1|\[::1\])(:\d+)?$/.test(host);

/**
 * The OAuth consent step: an app (an MCP client such as Claude) is asking for access to
 * the signed-in person's plans. The name is whatever the app calls itself, so the page
 * also shows what cannot be made up: the identifier it registered under and the
 * address the browser will be sent back to.
 */
export function Consent() {
  const status = useAccount((s) => s.status);
  const user = useAccount((s) => s.user);
  const params = new URLSearchParams(window.location.search);
  const clientId = params.get('client_id') ?? '';
  const redirectUri = params.get('redirect_uri') ?? '';
  const scopes = (params.get('scope') ?? '').split(/\s+/).filter(Boolean);
  /** What the app calls itself. */
  const [app, setApp] = useState<{ name: string } | null>(null);
  const [lookupFailed, setLookupFailed] = useState(false);
  const [busy, setBusy] = useState<'allow' | 'deny' | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (status !== 'signed-in' || !clientId) return;
    let live = true;
    void authClient.oauth2.publicClient({ query: { client_id: clientId } }).then((result) => {
      if (!live) return;
      const data = result.data as { client_name?: unknown } | null;
      if (result.error || !data) setLookupFailed(true);
      else setApp({ name: typeof data.client_name === 'string' && data.client_name.trim() ? data.client_name.trim().slice(0, 80) : 'An unnamed app' });
    });
    return () => {
      live = false;
    };
  }, [status, clientId]);

  const decide = async (accept: boolean) => {
    setBusy(accept ? 'allow' : 'deny');
    setError(null);
    const result = await authClient.oauth2.consent({ accept });
    const url = (result.data as { url?: string } | null)?.url;
    // On success the auth client sends the browser back to the app, with its code or with the refusal.
    if (result.error || !url) {
      setBusy(null);
      setError(authMessage(result.error, 'The answer could not be sent. The request may have expired: start again from the app.'));
    }
  };

  if (!clientId || !params.has('sig')) {
    return (
      <PageFrame>
        <h1 className="text-2xl font-semibold tracking-tight">Nothing is asking for access</h1>
        <p className="mt-2 text-sm leading-relaxed text-ink-2">This page appears when an AI tool asks to connect to your FIRESight account. To connect one, start from the tool, or see the instructions on your account page.</p>
        <Button className="mt-5" onClick={() => navigate('/account')}>
          Go to your account
        </Button>
      </PageFrame>
    );
  }

  if (status !== 'signed-in') {
    return (
      <PageFrame>
        <h1 className="text-2xl font-semibold tracking-tight">Sign in to continue</h1>
        <p className="mt-2 text-sm leading-relaxed text-ink-2">An app is asking to connect to a FIRESight account. Sign in to see which app it is and what it is asking for.</p>
        <Button variant="primary" className="mt-5" onClick={() => navigate(`/login${window.location.search}`)}>
          Sign in
        </Button>
      </PageFrame>
    );
  }

  const idHost = hostOf(clientId);
  const redirectHost = hostOf(redirectUri);
  const meanings = scopeMeanings(scopes);

  return (
    <PageFrame>
      <p className="eyebrow">Connect an app</p>
      {app ? (
        <h1 className="mt-2 text-2xl font-semibold tracking-tight text-balance" data-testid="consent-app">
          Allow {app.name} to work with your plans?
        </h1>
      ) : lookupFailed ? (
        <h1 className="mt-2 text-2xl font-semibold tracking-tight">Allow this app to work with your plans?</h1>
      ) : (
        <Skeleton className="mt-3 h-8 w-3/4" />
      )}
      <p className="mt-2 text-sm leading-relaxed text-ink-2">
        You are signed in as <span className="font-medium text-ink-1">{user?.email}</span>.
      </p>

      <dl className="mt-5 space-y-3 rounded-md border border-line bg-surface-1 p-4 text-sm">
        <div>
          <dt className="text-xs font-medium text-ink-3">It calls itself</dt>
          <dd className="mt-0.5">{app?.name ?? (lookupFailed ? 'Unknown: its details could not be looked up' : '…')}</dd>
        </div>
        <div>
          <dt className="text-xs font-medium text-ink-3">It is registered as</dt>
          <dd className="num mt-0.5 break-all text-xs">{idHost ? <><span className="text-sm font-medium">{idHost}</span> <span className="text-ink-3">({clientId})</span></> : clientId}</dd>
        </div>
        <div>
          <dt className="text-xs font-medium text-ink-3">After you answer, you are sent to</dt>
          <dd className="num mt-0.5 break-all text-xs">
            <span className="text-sm font-medium">{redirectHost ?? 'an address that could not be read'}</span>
            {redirectHost && isLoopback(redirectHost) && <span className="ml-1.5 font-sans text-ink-3">an app running on this computer</span>}
          </dd>
        </div>
      </dl>
      <p className="mt-2 flex items-start gap-1.5 text-xs leading-snug text-ink-3">
        <Icon name="info" size={12} className="mt-[2px]" />
        <span>The name is chosen by the app and proves nothing. Allow it only if you started this from an AI tool you trust and the addresses above are what you expect.</span>
      </p>

      <h2 className="mt-6 text-sm font-medium">It will be able to</h2>
      <ul className="mt-2 space-y-1.5 text-sm leading-relaxed text-ink-2">
        {meanings.map((meaning) => (
          <li key={meaning} className="flex gap-2">
            <Icon name="check" size={14} className="mt-1 text-ink-3" />
            <span>{meaning}</span>
          </li>
        ))}
        {meanings.length === 0 && <li>Confirm who you are, and nothing else.</li>}
      </ul>
      <p className="mt-3 text-xs leading-relaxed text-ink-3">
        It reaches only the plans in your account. Plans kept in this browser alone stay out of its reach. What it reads goes to that tool and the AI provider behind it. You can disconnect it at any time from your
        account page.
      </p>

      <div className="mt-5">
        <ErrorNote>{error}</ErrorNote>
      </div>
      <div className="mt-4 flex gap-2">
        <Button size="lg" className="flex-1" disabled={busy !== null} onClick={() => void decide(false)}>
          {busy === 'deny' ? 'One moment…' : 'Deny'}
        </Button>
        <Button size="lg" variant="primary" className="flex-1" disabled={busy !== null} onClick={() => void decide(true)}>
          {busy === 'allow' ? 'One moment…' : 'Allow'}
        </Button>
      </div>
    </PageFrame>
  );
}
