import { useEffect, useRef, useState } from 'react';
import { useProfileStore } from '@/state/profile-store';
import { ASSISTANT_MODELS, useSettings, type AssistantModel, type ThemeChoice } from '@/state/settings-store';
import { shareUrl } from '@/state/share';
import { toast, useUi } from '@/state/ui-store';
import { Button, Dialog, Field, Icon, SegmentedControl, Select, TextInput, count, cx } from '@/ui';
import { METHODOLOGY_URL, importPlanFile, importPlanText } from './actions';

export function SettingsDialog() {
  const open = useUi((s) => s.dialog === 'settings');
  const close = () => useUi.getState().openDialog(null);
  const { theme, apiKey, model, set } = useSettings();
  const [reveal, setReveal] = useState(false);
  return (
    <Dialog open={open} onClose={close} title="Settings">
      <div className="space-y-6">
        <Field label="Appearance">
          {({ id }) => (
            <SegmentedControl<ThemeChoice>
              id={id}
              aria-label="Appearance"
              value={theme}
              onChange={(v) => set({ theme: v })}
              options={[
                { value: 'system', label: (<><Icon name="monitor" size={13} /> System</>) },
                { value: 'light', label: (<><Icon name="sun" size={13} /> Light</>) },
                { value: 'dark', label: (<><Icon name="moon" size={13} /> Dark</>) },
              ]}
            />
          )}
        </Field>

        <div className="space-y-3 border-t border-line pt-5">
          <h3 className="text-sm font-medium">Assistant</h3>
          <Field
            label="Claude API key"
            hint="Optional. Kept only in this browser's local storage and sent only to the Claude API when you use the assistant. Clear the field to remove it."
          >
            {({ id, describedBy }) => (
              <div className="flex gap-1.5">
                <TextInput id={id} aria-describedby={describedBy} type={reveal ? 'text' : 'password'} placeholder="sk-ant-…" value={apiKey} onChange={(v) => set({ apiKey: v.trim() })} className="num w-full" />
                <Button icon="eye" aria-pressed={reveal} onClick={() => setReveal(!reveal)}>
                  {reveal ? 'Hide' : 'Show'}
                </Button>
              </div>
            )}
          </Field>
          <Field label="Model" hint={ASSISTANT_MODELS.find((m) => m.id === model)?.note}>
            {({ id }) => <Select<AssistantModel> id={id} value={model} onChange={(v) => set({ model: v })} options={ASSISTANT_MODELS.map((m) => ({ value: m.id, label: m.label }))} />}
          </Field>
        </div>

        <div className="border-t border-line pt-5 text-xs leading-relaxed text-ink-3">
          <p className="flex items-center gap-1.5 font-medium text-ink-2">
            <Icon name="lock" size={12} /> Your plan stays in this browser
          </p>
          <p className="mt-1">Simulation runs on your device. Plans are saved in this browser's storage and leave it only when you export a file or copy a share link.</p>
          <p className="mt-3">
            <a href={METHODOLOGY_URL} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 rounded-xs font-medium text-accent-ink hover:underline">
              About the model
              <Icon name="arrowRight" size={12} />
            </a>{' '}
            <span>How lives are simulated, what was checked against what, and what is left out.</span>
          </p>
        </div>
      </div>
    </Dialog>
  );
}

export function ShareDialog() {
  const open = useUi((s) => s.dialog === 'share');
  // The body mounts each time the dialog opens, so the link is always built from the current plan.
  return open ? <ShareDialogBody /> : null;
}

function ShareDialogBody() {
  const close = () => useUi.getState().openDialog(null);
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const s = useProfileStore.getState();
    if (!s.profile) return;
    let live = true;
    shareUrl({ v: 2, profile: s.profile, scenarios: s.scenarios, run: s.run }, window.location.origin + window.location.pathname).then(
      (u) => live && setUrl(u),
      () => live && setFailed(true),
    );
    return () => {
      live = false;
    };
  }, []);

  const copy = async () => {
    if (!url) return;
    try {
      await navigator.clipboard.writeText(url);
      toast({ tone: 'good', title: 'Link copied' });
    } catch {
      input.current?.select();
      toast({ tone: 'warn', title: 'Copy the selected link by hand', description: 'This browser did not allow access to the clipboard.' });
    }
  };

  return (
    <Dialog
      open
      onClose={close}
      title="Share this plan"
      description="The whole plan, with its scenarios, is packed into the link itself."
      footer={
        <>
          <Button onClick={close}>Done</Button>
          <Button variant="primary" icon="copy" disabled={!url} onClick={copy}>
            Copy link
          </Button>
        </>
      }
    >
      {failed ? (
        <p className="text-sm text-bad">This browser cannot compress the plan into a link. Export it as a file instead.</p>
      ) : (
        <>
          <label htmlFor="share-url" className="sr-only">
            Share link
          </label>
          <input
            id="share-url"
            ref={input}
            readOnly
            value={url ?? 'Building the link…'}
            onFocus={(e) => e.target.select()}
            className="num h-9 w-full truncate rounded-sm border border-line bg-surface-2 px-2.5 text-xs text-ink-2"
          />
          {url && <p className="num mt-1.5 text-xs text-ink-3">{count(url.length)} characters</p>}
          <ul className="mt-4 space-y-2 text-sm leading-relaxed text-ink-2">
            <li className="flex gap-2">
              <Icon name="lock" size={14} className="mt-1 text-ink-3" />
              <span>The plan sits after the # in the address, a part that browsers never send to any server, ours included.</span>
            </li>
            <li className="flex gap-2">
              <Icon name="users" size={14} className="mt-1 text-ink-3" />
              <span>Anyone with the link can read every figure in the plan, so send it only to people you would show the numbers to.</span>
            </li>
            <li className="flex gap-2">
              <Icon name="copy" size={14} className="mt-1 text-ink-3" />
              <span>Opening it makes a separate copy in their browser. Later changes on either side are not shared.</span>
            </li>
          </ul>
        </>
      )}
    </Dialog>
  );
}

export function ImportDialog() {
  const open = useUi((s) => s.dialog === 'import');
  const close = () => useUi.getState().openDialog(null);
  const [over, setOver] = useState(false);
  const [text, setText] = useState('');
  const file = useRef<HTMLInputElement>(null);

  const take = async (f: File | undefined) => {
    if (f && (await importPlanFile(f))) close();
  };

  return (
    <Dialog open={open} onClose={close} title="Import a plan" description="A FIRESight file from this version or from version 1. Version 1 plans are converted, and you will see what the conversion assumed.">
      <div
        onDragOver={(e) => {
          e.preventDefault();
          setOver(true);
        }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setOver(false);
          void take(e.dataTransfer.files[0]);
        }}
        className={cx('flex flex-col items-center gap-3 rounded-md border border-dashed px-6 py-8 text-center transition-colors', over ? 'border-accent bg-accent-soft' : 'border-line-strong')}
      >
        <Icon name="upload" size={20} className="text-ink-3" />
        <p className="text-sm text-ink-2">Drop a .json file here</p>
        <Button onClick={() => file.current?.click()}>Choose a file</Button>
        <input ref={file} type="file" accept=".json,application/json" className="sr-only" tabIndex={-1} aria-label="Plan file" onChange={(e) => void take(e.target.files?.[0])} />
      </div>
      <div className="mt-5">
        <label htmlFor="import-text" className="mb-1 block text-xs font-medium text-ink-2">
          Or paste the JSON
        </label>
        <textarea
          id="import-text"
          rows={4}
          value={text}
          onChange={(e) => setText(e.target.value)}
          spellCheck={false}
          className="num w-full resize-y rounded-sm border border-line bg-surface-2 px-2.5 py-2 text-xs hover:border-line-strong focus-visible:border-accent focus-visible:outline-offset-0"
        />
        <div className="mt-2 flex justify-end">
          <Button
            variant="primary"
            disabled={text.trim() === ''}
            onClick={() => {
              if (importPlanText(text)) {
                setText('');
                close();
              }
            }}
          >
            Import
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

/** Shown once after a version 1 plan is converted. */
export function MigrationNotesDialog() {
  const notes = useProfileStore((s) => s.migrationNotes);
  const name = useProfileStore((s) => s.profile?.name);
  const dismiss = useProfileStore((s) => s.dismissMigrationNotes);
  return (
    <Dialog
      open={notes !== null}
      onClose={dismiss}
      title="Converted from version 1"
      description={`"${name ?? 'Your plan'}" was brought into the new format. The two versions model some things differently, so the conversion made these choices. Review them in the editor.`}
      footer={
        <Button variant="primary" onClick={dismiss}>
          Review the plan
        </Button>
      }
    >
      <ol className="space-y-2">
        {(notes ?? []).map((note, i) => (
          <li key={i} className="grid grid-cols-[1.75rem_1fr] text-sm leading-relaxed text-ink-2">
            <span className="num pt-px text-xs text-ink-3">{String(i + 1).padStart(2, '0')}</span>
            <span>{note}</span>
          </li>
        ))}
      </ol>
    </Dialog>
  );
}
