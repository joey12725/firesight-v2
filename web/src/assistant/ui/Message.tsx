import { useState } from 'react';
import { Button, Disclosure, Icon, IconButton, cx } from '@/ui';
import type { AssistantItem, NoteItem, Part, ToolPart, UserItem } from '../conversation';
import { splitFollowups } from '../followups';
import { BasedOn } from './BasedOn';
import { ChangeCard } from './ChangeCard';
import { ChartCard } from './ChartCard';
import { Markdown } from './Markdown';
import { ToolRow } from './ToolRow';

const size = (bytes: number) => (bytes >= 1_000_000 ? `${(bytes / 1_000_000).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1000))} kB`);

export function UserMessage({ item }: { item: UserItem }) {
  return (
    <div className="flex flex-col items-end">
      <div className="max-w-[92%] rounded-md rounded-br-xs bg-surface-3 px-3 py-2 text-sm leading-relaxed whitespace-pre-wrap text-ink-1 [overflow-wrap:anywhere]">{item.text}</div>
      {item.attachments.length > 0 && (
        <ul aria-label="Attached files" className="mt-1.5 flex max-w-[92%] flex-wrap justify-end gap-1.5">
          {item.attachments.map((a, i) => (
            <li key={i} className="flex items-center gap-1.5 rounded-xs border border-line px-1.5 py-0.5 text-2xs text-ink-2">
              <Icon name="fileText" size={11} />
              <span className="max-w-40 truncate">{a.name}</span>
              <span className="num text-ink-3">{size(a.bytes)}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function Note({ item }: { item: NoteItem }) {
  return (
    <p role="note" className="border-y border-line py-2 text-xs leading-snug text-ink-3">
      {item.text}
    </p>
  );
}

function ToolBlock({ part }: { part: ToolPart }) {
  return (
    <>
      {part.display?.type !== 'chart' && <ToolRow part={part} />}
      {part.display?.type === 'change' && <ChangeCard part={part} display={part.display} />}
      {part.display?.type === 'chart' && <ChartCard display={part.display} />}
    </>
  );
}

function PartView({ part, streaming, last }: { part: Part; streaming: boolean; last: boolean }) {
  if (part.type === 'tool') return <ToolBlock part={part} />;
  if (part.type === 'thinking') {
    // While it streams, the summary of the model's reasoning doubles as a progress line.
    if (streaming && last) {
      return (
        <p className="mt-2 flex items-start gap-2 text-xs leading-snug text-ink-3 first:mt-0">
          <Icon name="activity" size={13} className="mt-[3px] animate-pulse" />
          <span className="line-clamp-2 min-w-0">{part.text.trim().split('\n').pop()}</span>
        </p>
      );
    }
    return (
      <Disclosure summary="Reasoning summary" className="mt-2 first:mt-0">
        <p className="border-l border-line pl-3 text-xs leading-relaxed whitespace-pre-wrap text-ink-3">{part.text.trim()}</p>
      </Disclosure>
    );
  }
  const { body } = splitFollowups(part.text);
  return body.trim() ? <Markdown source={body} className="mt-2 first:mt-0" /> : null;
}

const ENDING: Partial<Record<AssistantItem['status'], string>> = {
  stopped: 'Stopped.',
  truncated: 'The answer was cut off at the length limit. Ask it to continue.',
  step_limit: 'The assistant reached its limit of steps for one turn. Ask it to continue.',
  refused: "Claude's safety filters declined this request, so there is no answer. Rephrase it, or ask something else.",
};

interface AssistantMessageProps {
  item: AssistantItem;
  /** The newest answer offers regenerate and follow-ups. */
  latest: boolean;
  onRegenerate: () => void;
  onFollowup: (text: string) => void;
  onOpenSettings: () => void;
}

export function AssistantMessage({ item, latest, onRegenerate, onFollowup, onOpenSettings }: AssistantMessageProps) {
  const [copied, setCopied] = useState(false);
  const streaming = item.status === 'streaming';
  const tools = item.parts.filter((p): p is ToolPart => p.type === 'tool');
  const text = item.parts
    .flatMap((p) => (p.type === 'text' ? [splitFollowups(p.text).body] : []))
    .join('\n\n')
    .trim();

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  };

  return (
    <article aria-label="Assistant" aria-busy={streaming} className="min-w-0">
      {item.parts.map((part, i) => (
        <PartView key={part.type === 'tool' ? part.id : i} part={part} streaming={streaming} last={i === item.parts.length - 1} />
      ))}
      {streaming && item.parts.length === 0 && (
        <p className="flex items-center gap-2 text-xs text-ink-3">
          <Icon name="activity" size={13} className="animate-pulse" /> Thinking
        </p>
      )}

      {ENDING[item.status] && <p className="mt-2 text-xs leading-snug text-ink-3">{ENDING[item.status]}</p>}

      {item.status === 'error' && item.error && (
        <div role="alert" className="mt-2 rounded-md border border-line bg-surface-2 px-3 py-2.5 first:mt-0">
          <p className="flex items-center gap-1.5 text-sm font-medium text-ink-1">
            <Icon name="alertCircle" size={14} className="text-bad" /> {item.error.title}
          </p>
          <p className="mt-1 text-xs leading-relaxed text-ink-2">{item.error.message}</p>
          {item.error.detail && <p className="num mt-1.5 text-2xs leading-snug text-ink-3 [overflow-wrap:anywhere]">{item.error.detail}</p>}
          <div className="mt-2.5 flex gap-2">
            {latest && (
              <Button size="sm" icon="rotate" onClick={onRegenerate}>
                Try again
              </Button>
            )}
            {(item.error.kind === 'auth' || item.error.kind === 'permission' || item.error.kind === 'not_found' || item.error.kind === 'bad_request') && (
              <Button size="sm" variant="ghost" icon="settings" onClick={onOpenSettings}>
                Settings
              </Button>
            )}
          </div>
        </div>
      )}

      {!streaming && <BasedOn tools={tools} />}

      {!streaming && (text || latest) && item.status !== 'error' && (
        <div className={cx('mt-1.5 flex items-center gap-0.5', !latest && 'opacity-0 transition-opacity focus-within:opacity-100 hover:opacity-100 [article:hover_&]:opacity-100')}>
          {text && <IconButton size="sm" icon={copied ? 'check' : 'copy'} label={copied ? 'Copied' : 'Copy the answer'} onClick={copy} />}
          {latest && <IconButton size="sm" icon="rotate" label="Answer again" onClick={onRegenerate} />}
        </div>
      )}

      {latest && item.status === 'done' && item.followups.length > 0 && (
        <ul aria-label="Suggested follow-ups" className="mt-2.5 flex flex-col items-start gap-1.5">
          {item.followups.map((q) => (
            <li key={q} className="max-w-full">
              <button
                type="button"
                onClick={() => onFollowup(q)}
                className="max-w-full rounded-sm border border-line bg-surface-2 px-2.5 py-1.5 text-left text-xs leading-snug text-ink-2 hover:border-line-strong hover:text-ink-1"
              >
                {q}
              </button>
            </li>
          ))}
        </ul>
      )}
    </article>
  );
}
