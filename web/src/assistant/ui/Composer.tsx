import { useEffect, useRef, useState, type ClipboardEvent, type DragEvent, type KeyboardEvent } from 'react';
import { useUi } from '@/state/ui-store';
import { Button, Icon, IconButton, cx } from '@/ui';
import type { TransportMode } from '../agent/types';
import { ACCEPT, prepareAttachments, type AttachmentProblem, type PreparedAttachment } from '../attachments';

interface ComposerProps {
  busy: boolean;
  /** False when there is no way to reach the model; the box still keeps what is typed. */
  connected: boolean;
  mode: TransportMode;
  placeholder: string;
  footer: React.ReactNode;
  onSend: (text: string, attachments: PreparedAttachment[]) => void;
  onStop: () => void;
}

const size = (bytes: number) => (bytes >= 1_000_000 ? `${(bytes / 1_000_000).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1000))} kB`);

/** The message box: free text, attached images and PDFs, send and stop. */
export function Composer({ busy, connected, mode, placeholder, footer, onSend, onStop }: ComposerProps) {
  // The draft lives in the UI store so text handed over from elsewhere lands here.
  const draft = useUi((s) => s.assistantDraft) ?? '';
  const setDraft = useUi((s) => s.setAssistantDraft);
  const [attachments, setAttachments] = useState<PreparedAttachment[]>([]);
  const [problems, setProblems] = useState<AttachmentProblem[]>([]);
  const [dragging, setDragging] = useState(false);
  const box = useRef<HTMLTextAreaElement>(null);
  const picker = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  }, [draft]);

  const add = async (files: File[]) => {
    if (files.length === 0) return;
    const result = await prepareAttachments(files, attachments, mode);
    setAttachments((current) => [...current, ...result.added]);
    setProblems(result.problems);
  };

  const canSend = connected && !busy && (draft.trim().length > 0 || attachments.length > 0);
  const send = () => {
    if (!canSend) return;
    onSend(draft, attachments);
    setDraft(null);
    setAttachments([]);
    setProblems([]);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      send();
    } else if (e.key === 'Escape' && busy) {
      e.preventDefault();
      onStop();
    }
  };

  const onPaste = (e: ClipboardEvent<HTMLTextAreaElement>) => {
    const files = [...e.clipboardData.files];
    if (files.length > 0) {
      e.preventDefault();
      void add(files);
    }
  };

  const onDrop = (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setDragging(false);
    void add([...e.dataTransfer.files]);
  };

  return (
    <footer className="shrink-0 border-t border-line p-3">
      <div
        onDragOver={(e) => {
          if (e.dataTransfer.types.includes('Files')) {
            e.preventDefault();
            setDragging(true);
          }
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={onDrop}
        className={cx('rounded-sm border bg-surface-2 transition-colors duration-[var(--duration-fast)] focus-within:border-accent', dragging ? 'border-accent' : 'border-line hover:border-line-strong')}
      >
        {attachments.length > 0 && (
          <ul aria-label="Files to send" className="flex flex-wrap gap-1.5 px-2 pt-2">
            {attachments.map((a, i) => (
              <li key={`${a.name}-${i}`} className="flex items-center gap-1.5 rounded-xs bg-surface-3 py-0.5 pl-1.5 pr-0.5 text-2xs text-ink-2">
                <Icon name="fileText" size={11} />
                <span className="max-w-36 truncate">{a.name}</span>
                <span className="num text-ink-3">{size(a.bytes)}</span>
                <button type="button" aria-label={`Remove ${a.name}`} onClick={() => setAttachments((c) => c.filter((_, j) => j !== i))} className="rounded-xs p-0.5 text-ink-3 hover:bg-surface-2 hover:text-ink-1">
                  <Icon name="x" size={11} />
                </button>
              </li>
            ))}
          </ul>
        )}
        <label htmlFor="assistant-message" className="sr-only">
          Message to the assistant
        </label>
        <textarea
          ref={box}
          id="assistant-message"
          rows={2}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onKeyDown}
          onPaste={onPaste}
          placeholder={placeholder}
          aria-describedby="assistant-message-help"
          className="block max-h-[200px] w-full resize-none bg-transparent px-2.5 py-2 text-sm leading-relaxed text-ink-1 placeholder:text-ink-3 focus-visible:outline-none"
        />
        <div className="flex items-center gap-1 px-1.5 pb-1.5">
          <input
            ref={picker}
            type="file"
            accept={ACCEPT}
            multiple
            hidden
            onChange={(e) => {
              void add([...(e.target.files ?? [])]);
              e.target.value = '';
            }}
          />
          <IconButton size="sm" icon="upload" label="Attach an image or PDF" hint="pay stubs, statements" onClick={() => picker.current?.click()} />
          <span id="assistant-message-help" className="sr-only">
            Enter sends. Shift and Enter adds a line. Escape stops a running answer.
          </span>
          <span className="ml-auto" />
          {busy ? (
            <Button size="sm" onClick={onStop} aria-label="Stop the answer">
              <span aria-hidden className="size-2 rounded-[1px] bg-current" /> Stop
            </Button>
          ) : (
            <Button size="sm" variant="primary" disabled={!canSend} onClick={send}>
              Send
            </Button>
          )}
        </div>
      </div>

      {problems.length > 0 && (
        <ul role="alert" className="mt-2 space-y-1 text-xs leading-snug text-warn">
          {problems.map((p, i) => (
            <li key={i}>
              <span className="font-medium">{p.name}</span>: {p.reason}
            </li>
          ))}
        </ul>
      )}
      {attachments.length > 0 && (
        <p className="mt-2 text-2xs leading-snug text-ink-3">
          Files are read in this browser and sent to Claude with your message{mode === 'hosted' ? ", through this site's connection, which does not store them" : ', directly from this browser'}. Cover account numbers if you prefer; the assistant is told not to copy them.
        </p>
      )}
      <div className="mt-2 flex min-h-5 items-center justify-between gap-2 text-xs text-ink-3">{footer}</div>
    </footer>
  );
}
