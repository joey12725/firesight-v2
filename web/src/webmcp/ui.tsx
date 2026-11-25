// What a browser agent's request looks like to the user: a card in the corner of the
// page. It is deliberately not a modal dialog: a modal makes the rest of the page inert,
// for the user and for an agent driving the page alike, and a request from an agent is
// not urgent enough to take the page away. Loaded on demand (see host.tsx).
import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { ChangeCard } from '@/assistant/ui/ChangeCard';
import { useSettings } from '@/state/settings-store';
import { Button, Icon } from '@/ui';
import { answerApproval, answerConsent, useAgent, type PendingApproval } from './agent-store';

/**
 * Where the tray is drawn. Normally the page; while a modal dialog or sheet is open,
 * inside it, because everything outside an open modal is inert and painted beneath it.
 */
function useTrayHost(): Element | null {
  const [host, setHost] = useState<Element | null>(null);
  useEffect(() => {
    const find = () => setHost(document.querySelector('dialog[open]') ?? document.body);
    find();
    const observer = new MutationObserver(find);
    observer.observe(document.body, { subtree: true, attributes: true, attributeFilter: ['open'] });
    return () => observer.disconnect();
  }, []);
  return host;
}

function Consent() {
  const autoApply = useSettings((s) => s.agentAutoApply);
  const set = useSettings((s) => s.set);
  return (
    <section aria-labelledby="agent-consent-title" data-testid="agent-consent" className="p-3.5">
      <p className="eyebrow flex items-center gap-1.5">
        <Icon name="activity" size={12} /> Browser agent
      </p>
      <h2 id="agent-consent-title" className="mt-1.5 text-sm font-medium leading-snug text-ink-1">
        An AI agent in this browser wants to read and work with your plan
      </h2>
      <p className="mt-1.5 text-xs leading-relaxed text-ink-2">
        It can read the plan and its results, run simulations and open views. What it reads goes to that agent and the AI service behind it.{' '}
        {autoApply ? 'Its changes to the plan are applied without asking, as you chose in Settings; each can be undone.' : 'It cannot change the plan without your say: each change is shown to you first.'}
      </p>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <Button size="sm" variant="primary" icon="check" onClick={() => answerConsent(true)}>
          Allow for this session
        </Button>
        <Button
          size="sm"
          onClick={() => {
            set({ agentAccess: 'allow' });
            answerConsent(true);
          }}
        >
          Always allow
        </Button>
        <Button size="sm" variant="ghost" onClick={() => answerConsent(false)}>
          Don't allow
        </Button>
      </div>
      <p className="mt-2 text-2xs leading-snug text-ink-3">You can change this under Settings, Browser agents.</p>
    </section>
  );
}

function Approval({ approval, position, total }: { approval: PendingApproval; position: number; total: number }) {
  return (
    <section aria-label="A browser agent's request" data-testid="agent-approval" className="p-3.5">
      <p className="eyebrow flex items-center gap-1.5">
        <Icon name="activity" size={12} /> Browser agent
        <span className="num ml-auto normal-case tracking-normal text-ink-3">{total > 1 ? `${position} of ${total} · ` : ''}{approval.tool}</span>
      </p>
      {approval.kind === 'change' ? (
        <div className="mt-2">
          <ChangeCard part={{ id: approval.id }} display={{ type: 'change', proposal: approval.proposal, state: 'pending' }} onDecide={(decision) => answerApproval(approval.id, decision)} />
        </div>
      ) : (
        <>
          <h2 className="mt-1.5 text-sm font-medium leading-snug text-ink-1 [overflow-wrap:anywhere]">{approval.title}</h2>
          <p className="mt-1.5 text-xs leading-relaxed text-ink-2 [overflow-wrap:anywhere]">{approval.body}</p>
          {approval.details && approval.details.length > 0 && (
            <ul className="mt-2 list-disc space-y-0.5 pl-4 text-xs leading-snug text-ink-2 marker:text-ink-3">
              {approval.details.map((line, i) => (
                <li key={i} className="[overflow-wrap:anywhere]">
                  {line}
                </li>
              ))}
            </ul>
          )}
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <Button size="sm" variant="primary" icon="check" onClick={() => answerApproval(approval.id, { type: 'accept' })}>
              {approval.confirmLabel}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => answerApproval(approval.id, { type: 'decline' })}>
              Decline
            </Button>
          </div>
        </>
      )}
    </section>
  );
}

/** The consent prompt, or the oldest request waiting for an answer. */
export default function AgentTray() {
  const consentAsked = useAgent((s) => s.consentAsked);
  const approvals = useAgent((s) => s.approvals);
  const host = useTrayHost();
  const first = approvals[0];
  if (!host || (!consentAsked && !first)) return null;
  return createPortal(
    <div
      role="region"
      aria-live="polite"
      aria-label="Browser agent requests"
      className="scroll-thin fixed bottom-4 right-4 z-[70] max-h-[min(80vh,40rem)] w-[min(26rem,calc(100vw-2rem))] overflow-y-auto rounded-md border border-line-strong bg-surface-1 text-left shadow-overlay animate-pop-in"
    >
      {consentAsked ? <Consent /> : first ? <Approval key={first.id} approval={first} position={1} total={approvals.length} /> : null}
    </div>,
    host,
  );
}
