import { useCallback, useEffect, useState, type FormEvent, type ReactNode } from 'react';
import { CloudError, httpPlansApi } from '@/state/cloud-api';
import { unlinkAccount, useCloud } from '@/state/cloud-store';
import { downloadText } from '@/state/files';
import { loginPath, navigate } from '@/state/route';
import { toast } from '@/state/ui-store';
import { Button, Dialog, Field, Icon, Select, Skeleton, TabPanel, Tabs, Tag, TextInput, count } from '@/ui';
import { accountDeleted, refreshSession, signOut, useAccount } from '../account';
import { authClient, authMessage } from '../client';
import { deleteFailureMessage, readConfirmation, requestDeletion, type DeleteConfirmation } from '../delete-account';
import { DeleteAccountForm } from './DeleteAccount';
import { CodeBlock, ErrorNote, PageFrame, copyText, scopeMeanings, shortDate } from './shared';

function Section({ title, intro, children, testId }: { title: string; intro?: ReactNode; children: ReactNode; testId?: string }) {
  return (
    <section className="mt-10 border-t border-line-strong pt-5" data-testid={testId}>
      <h2 className="text-base font-medium tracking-tight">{title}</h2>
      {intro && <p className="mt-1 max-w-2xl text-sm leading-relaxed text-ink-2">{intro}</p>}
      <div className="mt-4">{children}</div>
    </section>
  );
}

// ---- Connect an AI -------------------------------------------------------------------

type ClientTab = 'claude' | 'claude-code' | 'cursor' | 'vscode' | 'other';
const CLIENT_TABS: { id: ClientTab; label: string }[] = [
  { id: 'claude', label: 'Claude' },
  { id: 'claude-code', label: 'Claude Code' },
  { id: 'cursor', label: 'Cursor' },
  { id: 'vscode', label: 'VS Code' },
  { id: 'other', label: 'Anything else' },
];
const KEY_PLACEHOLDER = 'fs_YOUR_KEY';

function ConnectAnAi({ mcpUrl }: { mcpUrl: string }) {
  const [tab, setTab] = useState<ClientTab>('claude');
  const cursor = JSON.stringify({ mcpServers: { firesight: { url: mcpUrl, headers: { Authorization: 'Bearer ${env:FIRESIGHT_API_KEY}' } } } }, null, 2);
  const vscode = JSON.stringify(
    {
      servers: { firesight: { type: 'http', url: mcpUrl, headers: { Authorization: 'Bearer ${input:firesight-key}' } } },
      inputs: [{ type: 'promptString', id: 'firesight-key', description: 'FIRESight API key', password: true }],
    },
    null,
    2,
  );
  const step = 'text-sm leading-relaxed text-ink-2';
  return (
    <Section
      testId="connect-ai"
      title="Connect an AI"
      intro="FIRESight has an MCP server: an AI tool that connects to it can create a plan in your account, change it and run simulations, and hands you a link that opens the plan here with every chart. It reaches the plans in your account, and nothing kept only in this browser."
    >
      <CodeBlock text={mcpUrl} label="the MCP address" />
      <Tabs tabs={CLIENT_TABS} active={tab} onChange={setTab} aria-label="AI tools" idBase="connect" className="mt-4" />
      <TabPanel idBase="connect" id={tab} className="space-y-3 pt-4">
        {tab === 'claude' && (
          <>
            <p className={step}>
              In Claude on the web or the desktop app, open <span className="font-medium text-ink-1">Customize → Connectors → Add custom connector</span>, paste the address above, and sign in here when Claude sends you
              to this site. No key is needed: you approve the connection on a page that names the app, and can end it below at any time.
            </p>
            <p className="text-xs leading-relaxed text-ink-3">Menus move. If yours differ, look for custom or remote MCP connectors in Claude's settings.</p>
          </>
        )}
        {tab === 'claude-code' && (
          <>
            <p className={step}>Add the server, then run /mcp inside Claude Code and choose FIRESight to sign in through the browser:</p>
            <CodeBlock text={`claude mcp add --transport http firesight ${mcpUrl}`} label="the command" />
            <p className={step}>Or skip the sign-in with an API key from the section below:</p>
            <CodeBlock text={`claude mcp add --transport http firesight ${mcpUrl} --header "Authorization: Bearer ${KEY_PLACEHOLDER}"`} label="the command" />
          </>
        )}
        {tab === 'cursor' && (
          <>
            <p className={step}>
              Add this to <span className="num text-xs">.cursor/mcp.json</span> in a project (or <span className="num text-xs">~/.cursor/mcp.json</span> for all of them), with an API key from the section below in the
              FIRESIGHT_API_KEY environment variable:
            </p>
            <CodeBlock text={cursor} label="the configuration" />
          </>
        )}
        {tab === 'vscode' && (
          <>
            <p className={step}>
              Add this to <span className="num text-xs">.vscode/mcp.json</span>. VS Code asks for the key once and keeps it out of the file:
            </p>
            <CodeBlock text={vscode} label="the configuration" />
          </>
        )}
        {tab === 'other' && (
          <p className={step}>
            Any MCP client that speaks Streamable HTTP can use the address above. One that supports OAuth signs you in through this site. Otherwise create an API key below and have the client send it with every
            request as the header <span className="num text-xs">Authorization: Bearer {KEY_PLACEHOLDER}</span>.
          </p>
        )}
      </TabPanel>
    </Section>
  );
}

// ---- API keys ------------------------------------------------------------------------

interface KeyRow {
  id: string;
  name: string | null;
  start: string | null;
  configId: string;
  createdAt: string | Date;
  lastRequest: string | Date | null;
  expiresAt: string | Date | null;
}

type KeyAccess = 'read' | 'read-write';
type KeyLife = '30' | '90' | '365' | 'never';
const DAY_SECONDS = 86_400;

function ApiKeys({ mcpUrl }: { mcpUrl: string | null }) {
  const [keys, setKeys] = useState<KeyRow[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [access, setAccess] = useState<KeyAccess>('read-write');
  const [life, setLife] = useState<KeyLife>('90');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** The one time a key's secret is on screen. */
  const [fresh, setFresh] = useState<{ name: string; key: string } | null>(null);

  // Bumped to list the keys again after one is created or revoked.
  const [listed, setListed] = useState(0);
  const load = useCallback(() => setListed((n) => n + 1), []);
  useEffect(() => {
    let live = true;
    void authClient.apiKey.list().then((result) => {
      if (!live) return;
      if (result.error || !result.data) {
        setLoadError(authMessage(result.error, 'Your keys could not be listed.'));
        return;
      }
      setLoadError(null);
      setKeys([...(result.data.apiKeys as KeyRow[])].sort((a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime()));
    });
    return () => {
      live = false;
    };
  }, [listed]);

  const create = async (event: FormEvent) => {
    event.preventDefault();
    const label = name.trim();
    if (!label || busy) return;
    setBusy(true);
    setError(null);
    const result = await authClient.apiKey.create({
      name: label,
      // The kind fixes what the key may do, on the server. Anything but 'read-write' is read-only.
      ...(access === 'read-write' ? { configId: 'read-write' } : {}),
      ...(life === 'never' ? {} : { expiresIn: Number(life) * DAY_SECONDS }),
    });
    setBusy(false);
    if (result.error || !result.data) {
      setError(authMessage(result.error, 'The key could not be created.'));
      return;
    }
    setFresh({ name: label, key: result.data.key });
    setName('');
    load();
  };

  const revoke = async (key: KeyRow) => {
    if (!window.confirm(`Revoke the key "${key.name ?? 'unnamed'}"? Anything using it stops working at once.`)) return;
    const result = await authClient.apiKey.delete({ keyId: key.id });
    if (result.error) {
      toast({ tone: 'bad', title: 'Could not revoke that key', description: authMessage(result.error, 'Try again.') });
      return;
    }
    if (fresh?.name === key.name) setFresh(null);
    toast({ title: `Revoked "${key.name ?? 'unnamed'}"` });
    load();
  };

  return (
    <Section
      testId="api-keys"
      title="API keys"
      intro="A key lets a tool that cannot sign in through the browser reach the plans in your account: Claude Code, an editor, a script of your own. Give each tool its own key so that you can revoke one without the others. A key is not a password for this website: it works on the MCP address only."
    >
      {fresh && (
        <div className="mb-5 rounded-md border border-accent bg-accent-soft p-4" data-testid="new-key">
          <p className="flex items-center gap-1.5 text-sm font-medium">
            <Icon name="key" size={14} /> "{fresh.name}" is ready. Copy it now: it is not shown again.
          </p>
          <div className="mt-2 flex gap-1.5">
            <input readOnly aria-label="The new API key" data-testid="new-key-secret" value={fresh.key} onFocus={(e) => e.target.select()} className="num h-8 min-w-0 flex-1 rounded-sm border border-line bg-surface-1 px-2 text-xs" />
            <Button icon="copy" onClick={() => void copyText(fresh.key, 'Key copied')}>
              Copy
            </Button>
          </div>
          {mcpUrl && (
            <>
              <p className="mt-3 text-xs text-ink-2">For Claude Code:</p>
              <CodeBlock className="mt-1 bg-surface-1" text={`claude mcp add --transport http firesight ${mcpUrl} --header "Authorization: Bearer ${fresh.key}"`} label="the command" />
            </>
          )}
          <p className="mt-3 text-xs leading-relaxed text-ink-2">Only a fingerprint of the key is stored here, so it cannot be shown again. If it is lost, revoke it and create another.</p>
          <Button size="sm" variant="ghost" className="-ml-2 mt-1" onClick={() => setFresh(null)}>
            I have copied it
          </Button>
        </div>
      )}

      <form onSubmit={(e) => void create(e)} className="grid gap-3 sm:grid-cols-[1fr_11rem_9rem_auto] sm:items-end">
        <Field label="Name">{({ id }) => <TextInput id={id} value={name} onChange={setName} maxLength={64} placeholder="Where it will be used, e.g. Claude Code" />}</Field>
        <Field label="Access">
          {({ id }) => (
            <Select<KeyAccess>
              id={id}
              value={access}
              onChange={setAccess}
              options={[
                { value: 'read-write', label: 'Read and write' },
                { value: 'read', label: 'Read only' },
              ]}
            />
          )}
        </Field>
        <Field label="Expires">
          {({ id }) => (
            <Select<KeyLife>
              id={id}
              value={life}
              onChange={setLife}
              options={[
                { value: '30', label: 'In 30 days' },
                { value: '90', label: 'In 90 days' },
                { value: '365', label: 'In a year' },
                { value: 'never', label: 'Never' },
              ]}
            />
          )}
        </Field>
        <Button type="submit" variant="primary" icon="plus" disabled={busy || name.trim() === ''}>
          Create key
        </Button>
      </form>
      <p className="mt-2 text-xs leading-relaxed text-ink-3">
        {access === 'read-write' ? 'Read and write: the tool can create plans in your account and change the synced ones, as well as read and simulate them.' : 'Read only: the tool can read your synced plans and run simulations, and cannot change anything.'}
      </p>
      <div className="mt-2">
        <ErrorNote>{error}</ErrorNote>
      </div>

      <div className="mt-5">
        {loadError ? (
          <ErrorNote>{loadError}</ErrorNote>
        ) : keys === null ? (
          <Skeleton className="h-10 w-full" />
        ) : keys.length === 0 ? (
          <p className="text-sm text-ink-3">No keys yet.</p>
        ) : (
          <ul className="border-t border-line" data-testid="key-list">
            {keys.map((key) => (
              <li key={key.id} className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-line py-2.5">
                <div className="min-w-0 flex-1">
                  <p className="flex flex-wrap items-center gap-2 text-sm font-medium">
                    <span className="truncate">{key.name ?? 'Unnamed key'}</span>
                    <Tag tone={key.configId === 'read-write' ? 'accent' : 'neutral'}>{key.configId === 'read-write' ? 'read and write' : 'read only'}</Tag>
                  </p>
                  <p className="mt-0.5 text-xs text-ink-3">
                    <span className="num">{key.start ?? 'fs_'}…</span> · created {shortDate(key.createdAt)} · last used {shortDate(key.lastRequest)} · {key.expiresAt ? `expires ${shortDate(key.expiresAt)}` : 'does not expire'}
                  </p>
                </div>
                <Button size="sm" variant="danger" onClick={() => void revoke(key)}>
                  Revoke
                </Button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Section>
  );
}

// ---- Connected apps ------------------------------------------------------------------

interface ConsentRow {
  id: string;
  clientId: string;
  scopes: string[];
  name: string;
}

function ConnectedApps() {
  const [apps, setApps] = useState<ConsentRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [listed, setListed] = useState(0);
  const load = useCallback(() => setListed((n) => n + 1), []);
  useEffect(() => {
    let live = true;
    void (async () => {
      const result = await authClient.oauth2.getConsents();
      if (result.error || !result.data) return { error: authMessage(result.error, 'Connected apps could not be listed.') };
      const rows = result.data as unknown as { id: string; clientId: string; scopes?: string[] | string }[];
      const apps = await Promise.all(
        rows.map(async (row) => {
          // The name is the app's own description of itself; the identifier is shown beside it.
          const client = await authClient.oauth2.publicClient({ query: { client_id: row.clientId } });
          const clientName = (client.data as { client_name?: unknown } | null)?.client_name;
          const scopes = Array.isArray(row.scopes) ? row.scopes : typeof row.scopes === 'string' ? row.scopes.split(/\s+/).filter(Boolean) : [];
          return { id: row.id, clientId: row.clientId, scopes, name: typeof clientName === 'string' && clientName.trim() ? clientName.trim().slice(0, 80) : 'An unnamed app' };
        }),
      );
      return { apps };
    })().then((outcome) => {
      if (!live) return;
      setError(outcome.error ?? null);
      if (outcome.apps) setApps(outcome.apps);
    });
    return () => {
      live = false;
    };
  }, [listed]);

  const disconnect = async (app: ConsentRow) => {
    if (!window.confirm(`Disconnect ${app.name}? It loses access to your plans at once and has to ask again to get it back.`)) return;
    const result = await authClient.oauth2.deleteConsent({ id: app.id });
    if (result.error) {
      toast({ tone: 'bad', title: 'Could not disconnect that app', description: authMessage(result.error, 'Try again.') });
      return;
    }
    toast({ title: `Disconnected ${app.name}` });
    load();
  };

  return (
    <Section testId="connected-apps" title="Connected apps" intro="AI tools you signed in to from here. Disconnecting one ends its access immediately.">
      {error ? (
        <ErrorNote>{error}</ErrorNote>
      ) : apps === null ? (
        <Skeleton className="h-10 w-full" />
      ) : apps.length === 0 ? (
        <p className="text-sm text-ink-3">No apps are connected.</p>
      ) : (
        <ul className="border-t border-line">
          {apps.map((app) => (
            <li key={app.id} className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-line py-2.5">
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium">{app.name}</p>
                <p className="num mt-0.5 break-all text-xs text-ink-3">{app.clientId}</p>
                <p className="mt-0.5 text-xs text-ink-2">{scopeMeanings(app.scopes).join(' · ') || 'Confirms who you are'}</p>
              </div>
              <Button size="sm" variant="danger" onClick={() => void disconnect(app)}>
                Disconnect
              </Button>
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}

// ---- Your data -----------------------------------------------------------------------

function YourData({ userId, email }: { userId: string; email: string }) {
  const remote = useCloud((s) => s.remote);
  const [exporting, setExporting] = useState(false);
  const [confirming, setConfirming] = useState(false);
  // How this account confirms a deletion: with its password, or (no password: GitHub or Google only) its email typed out.
  const [mode, setMode] = useState<DeleteConfirmation | 'loading' | 'unknown'>('loading');
  const [typed, setTyped] = useState('');
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const exportAll = async () => {
    setExporting(true);
    try {
      const data = await httpPlansApi(() => userId).exportAll();
      downloadText(`firesight-account-${new Date().toISOString().slice(0, 10)}.json`, `${JSON.stringify(data, null, 2)}\n`);
    } catch (e) {
      // Refused as not this account's session (another tab changed it): find out who is signed in now.
      if (e instanceof CloudError && e.kind === 'unauthenticated') void refreshSession();
      toast({ tone: 'bad', title: 'Could not export your plans', description: e instanceof CloudError ? e.message : 'Try again.' });
    } finally {
      setExporting(false);
    }
  };

  const askHow = useCallback(async () => {
    setMode('loading');
    setMode(await readConfirmation(authClient).catch(() => 'unknown' as const));
  }, []);

  const open = () => {
    setTyped('');
    setError(null);
    setConfirming(true);
    void askHow();
  };

  const remove = async () => {
    if (deleting || (mode !== 'password' && mode !== 'email')) return;
    setDeleting(true);
    setError(null);
    const failure = await requestDeletion(authClient, mode, typed, email).catch((e: unknown) => ({ message: e instanceof Error ? e.message : undefined }));
    if (failure) {
      setDeleting(false);
      setError(deleteFailureMessage(failure) ?? authMessage(failure, mode === 'password' ? 'The account could not be deleted. Check the password.' : 'The account could not be deleted.'));
      return;
    }
    // The plans went with the account. Copies in this browser stay, as ordinary local plans.
    await unlinkAccount(userId);
    accountDeleted();
    toast({ tone: 'good', title: 'Your account has been deleted', description: 'Everything it held on the server is gone. Plans you had opened in this browser are still here, and no longer synced.', duration: 12_000 });
    navigate('/', { replace: true });
  };

  return (
    <Section
      testId="your-data"
      title="Your data"
      intro={
        <>
          Your account holds {remote === null ? 'your synced plans' : remote.length === 0 ? 'no plans yet' : `${count(remote.length)} ${remote.length === 1 ? 'plan' : 'plans'}`}, your sign-in details, your API keys (as
          fingerprints) and your connected apps. Synced plans are encrypted where they are stored, with a key the server holds: the server can read them, and has to in order to simulate them for your AI tools.
          Assistant conversations are never synced; they stay in the browser they happened in.
        </>
      }
    >
      <div className="flex flex-wrap gap-2">
        <Button icon="download" disabled={exporting} onClick={() => void exportAll()}>
          {exporting ? 'Exporting…' : 'Export every plan in the account'}
        </Button>
        <Button variant="danger" icon="trash" onClick={open}>
          Delete account
        </Button>
      </div>
      <Dialog
        open={confirming}
        onClose={() => {
          if (!deleting) setConfirming(false);
        }}
        title="Delete your account?"
        description="This cannot be undone."
      >
        <DeleteAccountForm
          email={email}
          mode={mode}
          typed={typed}
          onTyped={setTyped}
          error={error}
          deleting={deleting}
          onCancel={() => setConfirming(false)}
          onSubmit={() => void remove()}
          onRetry={() => void askHow()}
        />
      </Dialog>
    </Section>
  );
}

// ---- the page ------------------------------------------------------------------------

export function Account() {
  const status = useAccount((s) => s.status);
  const user = useAccount((s) => s.user);
  const mcpUrl = useAccount((s) => s.config?.mcpUrl ?? null);

  useEffect(() => {
    if (status === 'signed-out') navigate(loginPath('/account'), { replace: true });
  }, [status]);
  if (status !== 'signed-in' || !user) return <PageFrame width="wide">{null}</PageFrame>;

  return (
    <PageFrame width="wide">
      <div className="flex flex-wrap items-start gap-3">
        <div className="min-w-0 flex-1">
          <p className="eyebrow">Account</p>
          <h1 className="mt-2 truncate text-2xl font-semibold tracking-tight">{user.name || user.email}</h1>
          <p className="mt-1 text-sm text-ink-2" data-testid="account-email">
            {user.email}
          </p>
        </div>
        <Button
          icon="logOut"
          onClick={() =>
            void signOut().then(() => {
              if (useAccount.getState().status === 'signed-out') {
                toast({ title: 'Signed out', description: 'Plans you had opened here stay in this browser. Syncing is paused until you sign in again.' });
                navigate('/');
              }
            })
          }
        >
          Sign out
        </Button>
      </div>
      {mcpUrl && <ConnectAnAi mcpUrl={mcpUrl} />}
      <ApiKeys mcpUrl={mcpUrl} />
      <ConnectedApps />
      <YourData userId={user.id} email={user.email} />
    </PageFrame>
  );
}
