import { Suspense, lazy, useEffect } from 'react';
import { AssistantPanel } from '@/assistant';
import { PlanLink } from '@/auth/PlanLink';
import { useAccount } from '@/auth/account';
import { PlanEditor } from '@/editor/PlanEditor';
import { GuidedSetup } from '@/onboarding/GuidedSetup';
import { Landing } from '@/onboarding/Landing';
import { useProfileStore } from '@/state/profile-store';
import { activePage, usePage } from '@/state/route';
import { useSettings } from '@/state/settings-store';
import { useUi } from '@/state/ui-store';
import { Sheet, Toaster, useMediaQuery } from '@/ui';
import { AgentHost } from '@/webmcp/host';
import { ImportDialog, MigrationNotesDialog, SettingsDialog, ShareDialog } from './dialogs';
import { ResultsWorkspace } from './ResultsWorkspace';
import { TopBar } from './TopBar';

// Sign-in, consent and the account page: loaded, with the auth client, only when one is opened.
const AccountPages = lazy(() => import('@/auth/pages'));
// The dialogs a synced plan can raise: loaded, with the sync code, once someone is signed in.
const CloudDialogs = lazy(() => import('./cloud-ui').then((m) => ({ default: m.CloudDialogs })));

function useGlobalShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.key.toLowerCase() !== 'z') return;
      // Plain text fields keep their own undo; figures and everything else use the plan's.
      const t = e.target as HTMLElement | null;
      if (t instanceof HTMLTextAreaElement) return;
      const store = useProfileStore.getState();
      if (!store.profile) return;
      // An account page has its own fields, and the plan is not on screen to watch an undo land.
      if (activePage().kind !== 'app') return;
      e.preventDefault();
      (document.activeElement as HTMLElement | null)?.blur?.();
      if (e.shiftKey) store.redo();
      else store.undo();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}

/**
 * Desktop (1280+): editor | results | assistant. Laptop and tablet (768+): editor |
 * results, assistant in a sheet. Phone: results only, editor and assistant in sheets.
 */
export function AppShell() {
  const hasPlan = useProfileStore((s) => s.profile !== null);
  const page = usePage();
  const signedIn = useAccount((s) => s.status === 'signed-in');
  const { editorOpen, assistantOpen, set } = useSettings();
  const sheet = useUi((s) => s.sheet);
  const openSheet = useUi((s) => s.openSheet);
  const twoColumn = useMediaQuery('(min-width: 768px)');
  const threeColumn = useMediaQuery('(min-width: 1280px)');
  useGlobalShortcuts();

  // A sheet left open makes no sense once its column is on screen.
  useEffect(() => {
    if ((sheet === 'editor' && twoColumn) || (sheet === 'assistant' && threeColumn)) openSheet(null);
  }, [sheet, twoColumn, threeColumn, openSheet]);

  return (
    <div className="flex h-full flex-col">
      <a href="#main" className="sr-only focus:not-sr-only focus:absolute focus:left-2 focus:top-2 focus:z-50 focus:rounded-sm focus:bg-surface-2 focus:px-3 focus:py-1.5 focus:text-sm">
        Skip to results
      </a>
      <TopBar compact={!twoColumn} />
      {page.kind !== 'app' ? (
        <div id="main" className="min-h-0 flex-1">
          {page.kind === 'plan' ? (
            <PlanLink id={page.id} view={page.view} />
          ) : (
            <Suspense fallback={null}>
              <AccountPages page={page} />
            </Suspense>
          )}
        </div>
      ) : hasPlan ? (
        <div className="flex min-h-0 flex-1">
          {twoColumn && editorOpen && (
            <div className="w-[var(--editor-width)] shrink-0 border-r border-line">
              <PlanEditor />
            </div>
          )}
          <div id="main" className="min-w-0 flex-1">
            <ResultsWorkspace />
          </div>
          {threeColumn && assistantOpen && (
            <div className="w-[var(--assistant-width)] shrink-0 border-l border-line">
              <AssistantPanel onClose={() => set({ assistantOpen: false })} />
            </div>
          )}
          <Sheet open={sheet === 'editor' && !twoColumn} onClose={() => openSheet(null)} title="Plan editor" side="left" bare>
            {sheet === 'editor' && <PlanEditor onClose={() => openSheet(null)} />}
          </Sheet>
          <Sheet open={sheet === 'assistant' && !threeColumn} onClose={() => openSheet(null)} title="Assistant" side="right" bare>
            {sheet === 'assistant' && <AssistantPanel onClose={() => openSheet(null)} />}
          </Sheet>
        </div>
      ) : (
        <div id="main" className="min-h-0 flex-1">
          <Landing />
        </div>
      )}
      <SettingsDialog />
      <ShareDialog />
      <ImportDialog />
      <GuidedSetup />
      <MigrationNotesDialog />
      {signedIn && (
        <Suspense fallback={null}>
          <CloudDialogs />
        </Suspense>
      )}
      {/* WebMCP: a browser agent's consent prompt and change cards. Renders nothing otherwise. */}
      <AgentHost />
      <Toaster />
    </div>
  );
}
