// The assistant panel itself. Loaded on demand by ../index.tsx so the model client and
// the conversation UI stay out of the app's first bundle.
import { useEffect, useRef } from 'react';
import { useProfileStore } from '@/state/profile-store';
import { useSettings } from '@/state/settings-store';
import { useUi } from '@/state/ui-store';
import { Icon, IconButton, Popover, Tag } from '@/ui';
import type { PreparedAttachment } from '../attachments';
import { modelLabel } from '../models';
import { REVIEW_PROMPT } from '../prompts';
import { getSession, useAssistant } from '../session';
import { Composer } from './Composer';
import { Connect, Welcome } from './Intro';
import { AssistantMessage, Note, UserMessage } from './Message';
import { Options } from './Options';

export interface AssistantPanelProps {
  /** Present when the panel can be closed (always, today). */
  onClose?: () => void;
}

export default function Panel({ onClose }: AssistantPanelProps) {
  const session = getSession();
  const householdId = useProfileStore((s) => s.meta?.id ?? null);
  const hasPlan = useProfileStore((s) => s.profile !== null);
  const emptyPlan = useProfileStore((s) => !s.profile || (s.profile.household.people.length === 0 && s.profile.incomes.length === 0 && s.profile.accounts.length === 0));
  const apiKey = useSettings((s) => s.apiKey);
  const model = useSettings((s) => s.model);
  const { items, busy, status, hosted, hostedChecked, notice, queued } = useAssistant();
  const openSettings = () => useUi.getState().openDialog('settings');

  const connected = Boolean(apiKey) || hosted.available;
  const mode = apiKey ? 'byok' : 'hosted';
  const activeModel = apiKey || hosted.models.includes(model) ? model : (hosted.models[0] ?? model);

  useEffect(() => {
    void session.load(householdId);
  }, [session, householdId]);
  // With a key of their own the hosted connection is not needed, so it is not probed.
  useEffect(() => {
    if (!apiKey) void session.checkHosted();
  }, [session, apiKey]);

  // Follow the conversation while the reader is at the bottom; leave them be otherwise.
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  useEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [items, status]);

  const send = (text: string, attachments: PreparedAttachment[] = []) => {
    pinned.current = true;
    void session.send({ text, attachments });
  };
  const lastAssistant = items.findLast((i) => i.kind === 'assistant')?.id;

  return (
    <aside aria-label="Assistant" className="flex h-full min-h-0 flex-col bg-surface-1">
      <header className="flex h-11 shrink-0 items-center gap-1 border-b border-line pl-4 pr-2">
        <Icon name="sparkles" size={14} className="text-accent" />
        <h2 className="eyebrow mr-auto pl-1">Assistant</h2>
        {connected && hasPlan && !emptyPlan && <IconButton icon="checkCircle" size="sm" label="Review my plan" disabled={busy} onClick={() => send(REVIEW_PROMPT)} />}
        <Popover label="Assistant options" placement="bottom-end" trigger={<IconButton icon="sliders" size="sm" label="Assistant options" />}>
          {(close) => <Options hosted={hosted} hasConversation={items.length > 0} onClear={() => void session.clear()} onOpenSettings={openSettings} close={close} />}
        </Popover>
        {onClose && <IconButton icon="x" size="sm" label="Close the assistant" onClick={onClose} />}
      </header>

      <div
        ref={scroller}
        onScroll={(e) => {
          const el = e.currentTarget;
          pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
        }}
        className="scroll-thin min-h-0 flex-1 overflow-y-auto px-4 py-5"
      >
        {items.length === 0 ? (
          !connected ? (
            <Connect checking={!hostedChecked} onOpenSettings={openSettings} />
          ) : !hasPlan ? (
            <p className="text-sm leading-relaxed text-ink-2">Open a plan, or start a blank one, and the assistant will work on it with you.</p>
          ) : (
            <Welcome emptyPlan={emptyPlan} onAsk={(text) => send(text)} onReview={() => send(REVIEW_PROMPT)} />
          )
        ) : (
          <div role="log" aria-label="Conversation" aria-live="polite" aria-relevant="additions" className="space-y-5">
            {items.map((item) =>
              item.kind === 'user' ? (
                <UserMessage key={item.id} item={item} />
              ) : item.kind === 'note' ? (
                <Note key={item.id} item={item} />
              ) : (
                <AssistantMessage key={item.id} item={item} latest={item.id === lastAssistant && !busy} onRegenerate={() => void session.regenerate()} onFollowup={(text) => send(text)} onOpenSettings={openSettings} />
              ),
            )}
          </div>
        )}

        {notice && (
          <div role="alert" className="mt-4 rounded-md border border-line bg-surface-2 px-3 py-2.5">
            <p className="flex items-center gap-1.5 text-sm font-medium text-ink-1">
              <Icon name="info" size={14} className="text-ink-3" /> {notice.title}
            </p>
            <p className="mt-1 text-xs leading-relaxed text-ink-2">{notice.message}</p>
            <div className="mt-2.5 flex gap-3 text-xs font-medium">
              <button type="button" onClick={openSettings} className="text-ink-2 underline decoration-line-strong underline-offset-2 hover:text-ink-1">
                Open Settings
              </button>
              {items.some((i) => i.kind === 'user') && (
                <button type="button" onClick={() => void session.regenerate()} className="text-ink-2 underline decoration-line-strong underline-offset-2 hover:text-ink-1">
                  Try again
                </button>
              )}
            </div>
          </div>
        )}
        {status && (
          <p role="status" className="mt-3 flex items-center gap-2 text-xs text-ink-3">
            <Icon name="activity" size={13} className="animate-pulse" /> {status}
          </p>
        )}
      </div>

      <Composer
        busy={busy}
        connected={connected && hasPlan}
        mode={mode}
        placeholder={!connected ? 'Connect Claude to send. What you type here is kept.' : emptyPlan ? 'Describe your household, income, savings and what you hope for.' : 'Ask a question, or try a what-if.'}
        onSend={send}
        onStop={() => session.stop()}
        footer={
          <>
            <button type="button" onClick={openSettings} className="min-w-0 truncate hover:text-ink-1">
              {connected ? `${modelLabel(activeModel)} · ${apiKey ? 'your API key' : 'hosted by this site'}` : 'Add a Claude API key in Settings'}
            </button>
            {queued > 0 ? <Tag>{queued} waiting</Tag> : <span className="shrink-0">Not financial advice</span>}
          </>
        }
      />
    </aside>
  );
}
