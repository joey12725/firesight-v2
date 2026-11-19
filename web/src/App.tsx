import { useEffect, useState } from 'react';
import { startEngine } from '@/engine';
import { openHousehold, refreshHouseholds, startAutosave } from '@/state/households';
import { normaliseLocation } from '@/state/route';
import { startRunController } from '@/state/run-store';
import { startThemeSync, useSettings } from '@/state/settings-store';
import { AppShell } from '@/shell/AppShell';
import { openSharedPlan } from '@/shell/actions';
import { Button, EmptyState, Skeleton } from '@/ui';

type Boot = { state: 'loading' } | { state: 'ready' } | { state: 'failed'; message: string };

let booting: Promise<void> | null = null;

/** Loads the engine, then restores the shared or last-opened household. Runs once. */
function boot(): Promise<void> {
  booting ??= (async () => {
    startThemeSync();
    normaliseLocation();
    await startEngine();
    startAutosave();
    startRunController();
    await refreshHouseholds();
    if (await openSharedPlan()) return;
    const last = useSettings.getState().lastHouseholdId;
    if (last) await openHousehold(last);
  })();
  return booting;
}

export function App() {
  const [status, setStatus] = useState<Boot>({ state: 'loading' });
  useEffect(() => {
    let live = true;
    boot().then(
      () => live && setStatus({ state: 'ready' }),
      (error: unknown) => live && setStatus({ state: 'failed', message: error instanceof Error ? error.message : String(error) }),
    );
    return () => {
      live = false;
    };
  }, []);

  if (status.state === 'ready') return <AppShell />;
  if (status.state === 'failed') {
    return (
      <EmptyState
        icon="alertCircle"
        tone="bad"
        title="The simulation engine could not load"
        action={
          <Button icon="rotate" onClick={() => window.location.reload()}>
            Reload
          </Button>
        }
      >
        FIRESight runs a WebAssembly engine in your browser, and it failed to start. <span className="num mt-2 block text-xs text-ink-3">{status.message}</span>
      </EmptyState>
    );
  }
  return (
    <div className="flex h-full flex-col" aria-busy="true" aria-label="Loading FIRESight">
      <div className="h-[var(--topbar-height)] border-b border-line bg-surface-1" />
      <div className="mx-auto w-full max-w-5xl space-y-4 px-8 pt-20">
        <Skeleton className="h-3 w-24" />
        <Skeleton className="h-9 w-2/3" />
        <Skeleton className="h-5 w-1/2" />
      </div>
    </div>
  );
}
