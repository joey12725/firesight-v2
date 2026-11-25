// The parts of the browser-agent surface that are in the first bundle: the top bar pill,
// the activity log and the Settings section. The consent prompt and the change cards
// (ui.tsx) load only when an agent has asked for something.
import { lazy, Suspense } from 'react';
import { useSettings } from '@/state/settings-store';
import { useUi } from '@/state/ui-store';
import { Disclosure, Field, Icon, Popover, SegmentedControl, Switch, Tag, cx } from '@/ui';
import { answerConsent, resetConsent, useAgent, type ActivityEntry, type AgentAccess } from './agent-store';

const Tray = lazy(() => import('./ui'));

/** Mount once, near the root. Renders nothing until an agent is waiting for the user. */
export function AgentHost() {
  const waiting = useAgent((s) => s.consentAsked || s.approvals.length > 0);
  if (!waiting) return null;
  return (
    <Suspense fallback={null}>
      <Tray />
    </Suspense>
  );
}

const OUTCOME: Record<ActivityEntry['outcome'], { text: string; tone: 'neutral' | 'good' | 'warn' | 'accent' }> = {
  running: { text: 'Running', tone: 'accent' },
  ok: { text: 'Done', tone: 'good' },
  error: { text: 'Error', tone: 'warn' },
  refused: { text: 'Not allowed', tone: 'neutral' },
  cancelled: { text: 'Cancelled', tone: 'neutral' },
};

const clock = (at: number) => new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
const took = (ms: number | null) => (ms === null ? '' : ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`);

/** The last calls a browser agent made, newest first: which tool, when, how it ended. Never what was sent or returned. */
export function AgentActivityLog({ className }: { className?: string }) {
  const log = useAgent((s) => s.log);
  if (log.length === 0) return <p className={cx('text-xs leading-snug text-ink-3', className)}>No agent has called FIRESight in this session.</p>;
  return (
    <ol className={cx('scroll-thin max-h-64 space-y-1 overflow-y-auto', className)} data-testid="agent-activity">
      {[...log].reverse().map((entry) => (
        <li key={entry.id} className="flex items-center gap-2 text-xs">
          <span className="num shrink-0 text-ink-3">{clock(entry.at)}</span>
          <span className="num min-w-0 flex-1 truncate text-ink-1" title={entry.code ? `${entry.tool}: ${entry.code}` : entry.tool}>
            {entry.tool}
          </span>
          <span className="num shrink-0 text-2xs text-ink-3">{took(entry.ms)}</span>
          <Tag tone={OUTCOME[entry.outcome].tone}>{OUTCOME[entry.outcome].text}</Tag>
        </li>
      ))}
    </ol>
  );
}

/** In the top bar while a browser agent is at work, and afterwards for the rest of the session, with the activity log behind it. */
export function AgentPill() {
  const inFlight = useAgent((s) => s.inFlight);
  const waiting = useAgent((s) => s.consentAsked || s.approvals.length > 0);
  const seen = useAgent((s) => s.log.length > 0);
  if (!seen && !waiting) return null;
  const text = waiting ? 'Agent is waiting for you' : inFlight > 0 ? 'Agent working' : 'Agent';
  return (
    <Popover
      label="Browser agent activity"
      placement="bottom-end"
      className="w-80 p-3"
      trigger={
        <button type="button" data-testid="agent-pill" className="flex h-8 items-center gap-2 rounded-sm px-2 text-xs text-ink-2 hover:bg-surface-3" aria-label={`Browser agent: ${text}`}>
          <span aria-hidden className={cx('size-1.5 rounded-full', waiting ? 'bg-warn animate-pulse' : inFlight > 0 ? 'bg-accent animate-pulse' : 'bg-ink-3')} />
          <Icon name="activity" size={13} className="md:hidden" />
          <span className="hidden md:inline">{text}</span>
        </button>
      }
    >
      {(close) => (
        <div>
          <p className="eyebrow">Browser agent</p>
          <p className="mt-1 text-xs leading-snug text-ink-2">An AI agent in this browser is using FIRESight's tools. These are its recent calls; what they sent and returned is not kept.</p>
          <AgentActivityLog className="mt-3" />
          <button
            type="button"
            onClick={() => {
              close();
              useUi.getState().openDialog('settings');
            }}
            className="mt-3 text-xs font-medium text-ink-2 underline decoration-line-strong underline-offset-2 hover:text-ink-1"
          >
            Agent access settings
          </button>
        </div>
      )}
    </Popover>
  );
}

const ACCESS: { value: AgentAccess; label: string }[] = [
  { value: 'ask', label: 'Ask first' },
  { value: 'allow', label: 'Always allow' },
  { value: 'off', label: 'Off' },
];

const ACCESS_HINT: Record<AgentAccess, string> = {
  ask: 'The first time an agent wants to read or change your plan, the page asks you. Your answer lasts until you close or reload the tab.',
  allow: 'Agents in this browser can read your plan and its results without a prompt. Changes still need your approval unless you turn that off below.',
  off: 'Agents are told that access is off and get nothing else.',
};

/** The "Browser agents" section of Settings. */
export function BrowserAgentsSettings() {
  const access = useSettings((s) => s.agentAccess);
  const autoApply = useSettings((s) => s.agentAutoApply);
  const set = useSettings((s) => s.set);
  const available = useAgent((s) => s.available);
  const toolCount = useAgent((s) => s.toolCount);
  const registrationError = useAgent((s) => s.registrationError);
  return (
    <div className="space-y-3 border-t border-line pt-5" data-testid="agent-settings">
      <h3 className="text-sm font-medium">Browser agents</h3>
      <p className="text-xs leading-relaxed text-ink-3">
        {registrationError
          ? `This browser offers WebMCP but did not accept FIRESight's tools (${registrationError}), so agents cannot use them.`
          : available
            ? `This browser offers WebMCP, so an AI agent running in it can call FIRESight's ${toolCount} tools on this page: read the plan and its results, run simulations, propose changes.`
            : 'Where a browser supports WebMCP, an AI agent running in it can call FIRESight\'s tools on this page. This browser does not offer it to this page, so nothing is exposed now; the choices below apply when one does.'}
      </p>
      <Field label="Agent access" hint={ACCESS_HINT[access]}>
        {({ id }) => (
          <SegmentedControl<AgentAccess>
            id={id}
            aria-label="Agent access"
            value={access}
            options={ACCESS}
            onChange={(value) => {
              set({ agentAccess: value });
              // Choosing "Always allow" answers a prompt that is on screen; anything else starts the question afresh.
              if (value === 'allow') answerConsent(true);
              else resetConsent();
            }}
          />
        )}
      </Field>
      <Field inline label="Apply agent changes without asking" hint="An agent's plan changes are applied as soon as they validate, with a notice. Each is one undo step. Off, you accept or decline every change on a card.">
        {({ id, describedBy }) => <Switch id={id} aria-describedby={describedBy} checked={autoApply} disabled={access === 'off'} onChange={(v) => set({ agentAutoApply: v })} />}
      </Field>
      <p className="text-xs leading-relaxed text-ink-3">What an agent reads through these tools leaves this browser: it goes to that agent and the AI service behind it, which FIRESight does not control. The Claude API key, share links and assistant conversations are never offered to agents.</p>
      <Disclosure summary="Recent agent activity">
        <AgentActivityLog />
      </Disclosure>
    </div>
  );
}
