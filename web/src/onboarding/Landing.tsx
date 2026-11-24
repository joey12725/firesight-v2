import { useMemo, useState } from 'react';
import { exampleProfiles } from '@/engine';
import { openHousehold, useHouseholds } from '@/state/households';
import { useUi } from '@/state/ui-store';
import { Button, Icon, cx, type IconName } from '@/ui';
import { METHODOLOGY_URL, openBlank, openExample } from '@/shell/actions';
import { askAssistant } from '@/assistant';

function Way({ icon, title, children, action }: { icon: IconName; title: string; children: React.ReactNode; action: React.ReactNode }) {
  return (
    <div className="flex flex-col border-t border-line-strong pt-4">
      <Icon name={icon} className="text-ink-3" />
      <h3 className="mt-3 text-base font-medium tracking-tight">{title}</h3>
      <div className="mt-1 flex-1 text-sm leading-relaxed text-ink-2">{children}</div>
      <div className="mt-4">{action}</div>
    </div>
  );
}

/** The first-run state: four ways into the app, and any households already saved here. */
export function Landing() {
  const examples = useMemo(() => exampleProfiles(), []);
  const saved = useHouseholds((s) => s.list);
  const openDialog = useUi((s) => s.openDialog);
  const [describe, setDescribe] = useState('');

  const handOff = () => {
    // The assistant needs a plan to draft into: start from the engine's blank profile, then send the
    // description as the first message of that household's conversation. When no model is connected the
    // message still shows in the panel, with how to connect, and can be sent again from there.
    const text = describe.trim();
    if (!text) return;
    openBlank();
    askAssistant(text, { source: 'landing', request: 'Draft a first plan for this household from the description, asking only for what is essential and missing.' });
  };

  return (
    <main className="scroll-thin h-full overflow-y-auto bg-surface-0">
      <div className="mx-auto max-w-5xl px-5 pb-20 pt-12 sm:px-8 sm:pt-20">
        <p className="eyebrow">FIRESight</p>
        <h1 className="mt-3 max-w-3xl text-3xl font-semibold tracking-tight text-balance">When can work become optional, and how sure is that?</h1>
        <p className="mt-4 max-w-2xl text-lg leading-relaxed text-pretty text-ink-2">
          FIRESight lives your financial plan forward tens of thousands of times: through bull markets and crashes, layoffs, long lives and short ones. Then it shows you the whole range, not one hopeful
          line. Everything runs on your device.
        </p>

        {saved.length > 0 && (
          <section aria-labelledby="saved-title" className="mt-12">
            <h2 id="saved-title" className="eyebrow">
              Pick up where you left off
            </h2>
            <ul className="mt-2 border-t border-line">
              {saved.slice(0, 5).map((h) => (
                <li key={h.id}>
                  <button type="button" onClick={() => void openHousehold(h.id)} className="group flex w-full items-center gap-3 border-b border-line py-2.5 text-left hover:bg-surface-3/50">
                    <span className="min-w-0 flex-1 truncate text-sm font-medium">{h.name}</span>
                    <span className="num text-xs text-ink-3">{new Date(h.updatedAt).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })}</span>
                    <Icon name="arrowRight" size={14} className="text-ink-3 group-hover:text-accent" />
                  </button>
                </li>
              ))}
            </ul>
          </section>
        )}

        <section aria-labelledby="demo-title" className="mt-12">
          <h2 id="demo-title" className="eyebrow">
            Explore a demo household
          </h2>
          <p className="mt-1 text-sm text-ink-2">Made-up households with real detail. Open one to see every view on a finished plan, then change anything: it re-runs as you type.</p>
          <ul className={cx('mt-3 grid gap-3', examples.length % 3 === 0 ? 'md:grid-cols-3' : 'md:grid-cols-2')}>
            {examples.map((ex) => (
              <li key={ex.slug}>
                <button
                  type="button"
                  data-testid={`demo-${ex.slug}`}
                  onClick={() => openExample(ex.slug)}
                  className="group flex h-full w-full flex-col rounded-md border border-line bg-surface-1 px-4 py-3.5 text-left transition-colors duration-[var(--duration-fast)] hover:border-line-strong hover:bg-surface-2"
                >
                  <span className="flex items-center justify-between text-base font-medium tracking-tight">
                    {ex.title}
                    <Icon name="arrowRight" size={14} className="text-ink-3 transition-transform duration-[var(--duration-base)] group-hover:translate-x-0.5 group-hover:text-accent" />
                  </span>
                  <span className="mt-1 text-sm leading-relaxed text-ink-2">{ex.summary}</span>
                </button>
              </li>
            ))}
          </ul>
        </section>

        <section aria-labelledby="own-title" className="mt-12">
          <h2 id="own-title" className="eyebrow">
            Or start with your own
          </h2>
          <div className="mt-2 grid gap-x-8 gap-y-8 md:grid-cols-3">
            <Way
              icon="pencil"
              title="Answer seven questions"
              action={
                <Button variant="primary" onClick={() => openDialog('setup')}>
                  Start the guided setup
                </Button>
              }
            >
              Age, income, spending and savings: enough for a first picture in two minutes. Refine anything afterwards.
            </Way>
            <Way
              icon="upload"
              title="Import a file"
              action={
                <div className="flex gap-2">
                  <Button onClick={() => openDialog('import')}>Import a plan</Button>
                  <Button variant="ghost" onClick={openBlank}>
                    Start blank
                  </Button>
                </div>
              }
            >
              Open a plan you exported, or one from FIRESight version 1. Old plans are converted, with a list of what the conversion assumed.
            </Way>
            <Way
              icon="sparkles"
              title="Describe your situation to Claude"
              action={
                <Button disabled={describe.trim() === ''} onClick={handOff}>
                  Hand this to the assistant
                </Button>
              }
            >
              <label htmlFor="describe" className="sr-only">
                Describe your situation
              </label>
              <textarea
                id="describe"
                rows={3}
                value={describe}
                onChange={(e) => setDescribe(e.target.value)}
                placeholder="We're 38 and 41 in Denver, two kids, about $210k between us…"
                className="mt-1 w-full resize-none rounded-sm border border-line bg-surface-2 px-2.5 py-2 text-sm text-ink-1 hover:border-line-strong focus-visible:border-accent focus-visible:outline-offset-0"
              />
            </Way>
          </div>
        </section>

        <p className="mt-16 flex flex-wrap items-center gap-x-1.5 gap-y-1 border-t border-line pt-4 text-xs text-ink-3">
          <Icon name="lock" size={12} />
          <span>No account. Your plan is stored in this browser and is never uploaded.</span>
          <a href={METHODOLOGY_URL} target="_blank" rel="noreferrer" className="ml-auto rounded-xs font-medium text-ink-2 hover:text-accent-ink hover:underline">
            About the model
          </a>
        </p>
      </div>
    </main>
  );
}
