import { useState } from 'react';
import type { PatchOp } from '@/engine/types';
import { useProfileStore } from '@/state/profile-store';
import { Button, Icon, Tag, TextInput, cx } from '@/ui';
import type { ToolPart } from '../conversation';
import { getSession } from '../session';
import type { Assumption, ChangeEntry, ProposalState, ToolDisplay } from '../tools/types';
import { describeObject, formatValue, lastSegment } from './values';

type ChangeDisplay = Extract<ToolDisplay, { type: 'change' }>;

const STATE: Record<Exclude<ProposalState, 'pending'>, { text: string; tone: 'good' | 'neutral' | 'warn' }> = {
  accepted: { text: 'Accepted', tone: 'good' },
  edited: { text: 'Accepted with your edits', tone: 'good' },
  auto: { text: 'Applied automatically', tone: 'good' },
  declined: { text: 'Declined', tone: 'neutral' },
  failed: { text: 'Could not be applied', tone: 'warn' },
  cancelled: { text: 'Not answered', tone: 'neutral' },
};

const BASIS: Record<Assumption['basis'], { text: string; tone: 'neutral' | 'warn' | 'cool' }> = {
  stated: { text: 'You said', tone: 'neutral' },
  inferred: { text: 'Inferred', tone: 'warn' },
  default: { text: 'Default', tone: 'cool' },
};

const editable = (entry: ChangeEntry) => entry.op === 'set' && (typeof entry.after === 'number' || typeof entry.after === 'string' || typeof entry.after === 'boolean');

function Entry({ entry, draft, onDraft }: { entry: ChangeEntry; draft?: string; onDraft?: (text: string) => void }) {
  const key = lastSegment(entry.path);
  const isObject = entry.after !== null && typeof entry.after === 'object' && !Array.isArray(entry.after);
  return (
    <li className="py-2 first:pt-0 last:pb-0">
      <p className="text-xs text-ink-2">
        {entry.op === 'append' ? 'Add: ' : entry.op === 'remove' ? 'Remove: ' : ''}
        <span className="text-ink-1">{entry.label}</span>
      </p>
      {entry.op === 'remove' ? (
        <p className="num mt-0.5 text-xs text-ink-3 line-through">{formatValue(key, entry.before)}</p>
      ) : entry.op === 'append' && isObject ? (
        <dl className="mt-1 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs">
          {describeObject(entry.after as Record<string, unknown>).map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-ink-3">{k}</dt>
              <dd className="num min-w-0 text-ink-1 [overflow-wrap:anywhere]">{v}</dd>
            </div>
          ))}
        </dl>
      ) : (
        <p className="num mt-0.5 flex flex-wrap items-baseline gap-x-1.5 text-xs">
          {entry.op === 'set' && (
            <>
              <span className="text-ink-3">{formatValue(key, entry.before)}</span>
              <Icon name="arrowRight" size={11} className="translate-y-px text-ink-3" aria-label="becomes" />
            </>
          )}
          {onDraft && draft !== undefined ? (
            <TextInput aria-label={`New value for ${entry.label}`} value={draft} onChange={onDraft} className="num h-7 w-36 px-1.5 text-xs" />
          ) : (
            <span className="font-medium text-ink-1 [overflow-wrap:anywhere]">{formatValue(key, entry.after)}</span>
          )}
        </p>
      )}
    </li>
  );
}

function parseDraft(text: string, original: unknown): { ok: true; value: unknown } | { ok: false } {
  if (typeof original === 'number') {
    const cleaned = text.replace(/[$,\s]/g, '');
    const percent = cleaned.endsWith('%');
    const n = Number(percent ? cleaned.slice(0, -1) : cleaned);
    return cleaned !== '' && Number.isFinite(n) ? { ok: true, value: percent ? n / 100 : n } : { ok: false };
  }
  if (typeof original === 'boolean') {
    const t = text.trim().toLowerCase();
    return t === 'yes' || t === 'true' ? { ok: true, value: true } : t === 'no' || t === 'false' ? { ok: true, value: false } : { ok: false };
  }
  return { ok: true, value: text };
}

/** A proposed change to the plan: each edit as before and after, the assumptions behind it, and the decision. */
export function ChangeCard({ part, display }: { part: ToolPart; display: ChangeDisplay }) {
  const { proposal, state } = display;
  const pending = state === 'pending';
  const [editing, setEditing] = useState(false);
  const [drafts, setDrafts] = useState<Record<number, string>>({});
  const [error, setError] = useState<string | null>(null);
  const session = getSession();

  const startEditing = () => {
    setDrafts(Object.fromEntries(proposal.entries.flatMap((e, i) => (editable(e) ? [[i, String(e.after)]] : []))));
    setError(null);
    setEditing(true);
  };

  const acceptEdited = () => {
    const ops: PatchOp[] = [];
    for (const [i, op] of proposal.ops.entries()) {
      const entry = proposal.entries[i];
      const draft = drafts[i];
      if (entry && draft !== undefined && op.op === 'set') {
        const parsed = parseDraft(draft, entry.after);
        if (!parsed.ok) return setError(`"${draft}" is not a valid value for ${entry.label}.`);
        ops.push({ ...op, value: parsed.value });
      } else ops.push(op);
    }
    const preview = useProfileStore.getState().preview(ops);
    if ('error' in preview) return setError(preview.error);
    session.decide(part.id, { type: 'accept_edited', ops });
  };

  const anyEditable = proposal.entries.some(editable);
  return (
    <section aria-label={`Proposed change: ${proposal.summary}`} className="mt-3 overflow-hidden rounded-md border border-line bg-surface-2 first:mt-0">
      <header className="flex items-start gap-2 border-b border-line px-3 py-2">
        <div className="min-w-0 flex-1">
          <p className="eyebrow">{pending ? 'Proposed change' : 'Change to the plan'}</p>
          <h4 className="mt-0.5 text-sm font-medium leading-snug text-ink-1">{proposal.summary}</h4>
        </div>
        {!pending && (
          <Tag tone={STATE[state].tone} icon={STATE[state].tone === 'good' ? 'check' : undefined} className="mt-0.5">
            {STATE[state].text}
          </Tag>
        )}
      </header>

      <ul className="divide-y divide-line px-3 py-2.5">
        {proposal.entries.map((entry, i) => (
          <Entry key={i} entry={entry} draft={editing ? drafts[i] : undefined} onDraft={editing && editable(entry) ? (text) => setDrafts((d) => ({ ...d, [i]: text })) : undefined} />
        ))}
      </ul>

      {proposal.assumptions.length > 0 && (
        <div className="border-t border-line px-3 py-2.5">
          <p className="eyebrow">Where the values came from</p>
          <ul className="mt-1.5 space-y-1.5">
            {proposal.assumptions.map((a, i) => (
              <li key={i} className="flex items-start gap-2 text-xs leading-snug">
                <Tag tone={BASIS[a.basis].tone} className="mt-px shrink-0">
                  {BASIS[a.basis].text}
                </Tag>
                <span className="min-w-0 text-ink-2">
                  <span className="text-ink-1">{a.field}</span>: {a.value}
                  {a.basis !== 'stated' && a.confidence !== 'high' && <span className={cx('ml-1', a.confidence === 'low' ? 'text-warn' : 'text-ink-3')}>({a.confidence} confidence)</span>}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {proposal.newWarnings.length > 0 && (
        <div className="border-t border-line px-3 py-2.5 text-xs leading-snug text-ink-2">
          <p className="flex items-center gap-1.5 font-medium text-warn">
            <Icon name="alertTriangle" size={12} /> The plan check would flag
          </p>
          <ul className="mt-1 list-disc space-y-0.5 pl-4 marker:text-ink-3">
            {proposal.newWarnings.slice(0, 4).map((w) => (
              <li key={w}>{w.replace(/^[a-z0-9_.-]+ at \S+: /i, '')}</li>
            ))}
          </ul>
        </div>
      )}

      {pending && (
        <footer className="border-t border-line px-3 py-2.5">
          {error && (
            <p role="alert" className="mb-2 text-xs leading-snug text-bad">
              {error}
            </p>
          )}
          <div className="flex flex-wrap items-center gap-2">
            {editing ? (
              <>
                <Button size="sm" variant="primary" icon="check" onClick={acceptEdited}>
                  Accept with edits
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
                  Cancel edits
                </Button>
              </>
            ) : (
              <>
                <Button size="sm" variant="primary" icon="check" onClick={() => session.decide(part.id, { type: 'accept' })}>
                  Accept
                </Button>
                {anyEditable && (
                  <Button size="sm" icon="pencil" onClick={startEditing}>
                    Edit
                  </Button>
                )}
                <Button size="sm" variant="ghost" onClick={() => session.decide(part.id, { type: 'decline' })}>
                  Decline
                </Button>
              </>
            )}
          </div>
          <p className="mt-2 text-2xs leading-snug text-ink-3">Nothing changes until you accept. Accepted changes can be undone like any other edit.</p>
        </footer>
      )}
    </section>
  );
}
