import { useId, useState } from 'react';
import { Icon, cx, type IconName } from '@/ui';
import type { ToolPart, ToolStatus } from '../conversation';

const STATUS: Record<ToolStatus, { icon: IconName; tone: string; label: string }> = {
  drafting: { icon: 'activity', tone: 'text-ink-3', label: 'Preparing' },
  running: { icon: 'activity', tone: 'text-accent', label: 'Running' },
  done: { icon: 'check', tone: 'text-good', label: 'Done' },
  error: { icon: 'alertTriangle', tone: 'text-warn', label: 'Did not work' },
  cancelled: { icon: 'minus', tone: 'text-ink-3', label: 'Stopped' },
};

function pretty(value: unknown): string {
  if (typeof value === 'string') {
    try {
      return JSON.stringify(JSON.parse(value), null, 2);
    } catch {
      return value;
    }
  }
  return JSON.stringify(value, null, 2) ?? '';
}

const clip = (text: string, max = 6000) => (text.length > max ? `${text.slice(0, max)}\n… ${text.length - max} more characters` : text);

/** One line of tool activity. Expands to show exactly what was asked and what came back. */
export function ToolRow({ part }: { part: ToolPart }) {
  const [open, setOpen] = useState(false);
  const id = useId();
  const status = STATUS[part.status];
  const live = part.status === 'running' || part.status === 'drafting';
  return (
    <div className="mt-2 first:mt-0">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen(!open)}
        className="group flex w-full items-start gap-2 rounded-xs py-0.5 text-left text-xs text-ink-2 hover:text-ink-1"
      >
        <Icon name={status.icon} size={13} className={cx('mt-[3px]', status.tone, live && 'animate-pulse')} aria-label={status.label} />
        <span className="min-w-0 flex-1 leading-snug">{part.title}</span>
        <Icon name="chevronRight" size={12} className={cx('mt-[3px] text-ink-3 transition-transform duration-[var(--duration-fast)] group-hover:text-ink-2', open && 'rotate-90')} />
      </button>
      {open && (
        <div id={id} className="mt-1.5 ml-[21px] space-y-2 border-l border-line pl-3">
          <Detail label={`Asked (${part.name})`} text={part.input === undefined ? 'The request is still being written.' : pretty(part.input)} />
          {part.result !== undefined && <Detail label={part.status === 'error' ? 'Returned an error' : 'Returned'} text={pretty(part.result)} />}
        </div>
      )}
    </div>
  );
}

function Detail({ label, text }: { label: string; text: string }) {
  return (
    <div>
      <p className="eyebrow">{label}</p>
      <pre tabIndex={0} className="scroll-thin num mt-1 max-h-56 overflow-auto rounded-xs bg-surface-3 p-2 text-2xs leading-relaxed whitespace-pre-wrap text-ink-2 [overflow-wrap:anywhere]">
        {clip(text)}
      </pre>
    </div>
  );
}
