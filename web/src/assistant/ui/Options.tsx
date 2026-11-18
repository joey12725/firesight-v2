import { useState } from 'react';
import { useSettings } from '@/state/settings-store';
import { Button, Field, Switch, TextInput } from '@/ui';
import type { HostedInfo } from '../transport';

interface OptionsProps {
  hosted: HostedInfo;
  hasConversation: boolean;
  onClear: () => void;
  onOpenSettings: () => void;
  close: () => void;
}

/** Assistant settings that belong next to the conversation. Key and model live in Settings. */
export function Options({ hosted, hasConversation, onClear, onOpenSettings, close }: OptionsProps) {
  const autoAccept = useSettings((s) => s.assistantAutoAccept);
  const accessCode = useSettings((s) => s.hostedAccessCode);
  const apiKey = useSettings((s) => s.apiKey);
  const set = useSettings((s) => s.set);
  const [confirming, setConfirming] = useState(false);
  return (
    <div className="w-72 space-y-4 p-3.5">
      <Field inline label="Apply changes without asking" hint="The assistant's proposed plan changes are applied as soon as they validate. Each is still shown, and can be undone.">
        {({ id, describedBy }) => <Switch id={id} aria-describedby={describedBy} checked={autoAccept} onChange={(v) => set({ assistantAutoAccept: v })} />}
      </Field>

      {hosted.available && hosted.accessCodeRequired && !apiKey && (
        <Field label="Access code" hint="This site asks for a code before using its hosted connection. It is kept in this browser.">
          {({ id, describedBy }) => <TextInput id={id} aria-describedby={describedBy} type="password" value={accessCode} onChange={(v) => set({ hostedAccessCode: v.trim() })} className="num w-full" />}
        </Field>
      )}

      <div className="border-t border-line pt-3 text-xs leading-relaxed text-ink-3">
        <p>{apiKey ? 'Connected with your own API key, sent only to Anthropic from this browser.' : hosted.available ? "Connected through this site's hosted connection. Your own key, if you add one in Settings, takes precedence." : 'Not connected.'}</p>
        <button
          type="button"
          onClick={() => {
            close();
            onOpenSettings();
          }}
          className="mt-1 font-medium text-ink-2 underline decoration-line-strong underline-offset-2 hover:text-ink-1"
        >
          API key and model
        </button>
      </div>

      <div className="border-t border-line pt-3">
        {confirming ? (
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              variant="danger"
              icon="trash"
              onClick={() => {
                onClear();
                close();
              }}
            >
              Clear it
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setConfirming(false)}>
              Keep
            </Button>
          </div>
        ) : (
          <Button size="sm" variant="ghost" icon="trash" disabled={!hasConversation} onClick={() => setConfirming(true)}>
            Clear conversation
          </Button>
        )}
        <p className="mt-1.5 text-2xs leading-snug text-ink-3">The conversation is saved with this plan in this browser. Clearing it does not change the plan.</p>
      </div>
    </div>
  );
}
