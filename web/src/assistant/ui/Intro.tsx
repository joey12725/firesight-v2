import type { ReactNode } from 'react';
import { Button, Icon, Skeleton } from '@/ui';
import { EQUITY_PROMPT } from '../prompts';

const CONSOLE_KEYS_URL = 'https://console.anthropic.com/settings/keys';

function Way({ title, children }: { title: string; children: ReactNode }) {
  return (
    <li className="border-t border-line pt-3.5">
      <h4 className="text-sm font-medium text-ink-1">{title}</h4>
      <div className="mt-1 space-y-2 text-sm leading-relaxed text-ink-2">{children}</div>
    </li>
  );
}

/** Shown when there is no way to reach Claude: what the assistant is and the two ways to turn it on. */
export function Connect({ checking, onOpenSettings }: { checking: boolean; onOpenSettings: () => void }) {
  return (
    <div>
      <h3 className="text-lg font-semibold tracking-tight text-balance">Connect Claude to use the assistant</h3>
      <p className="mt-2 text-sm leading-relaxed text-ink-2">
        The assistant drafts a plan from your description or documents, runs the simulator to answer what-if questions, and explains results. It runs on Claude, Anthropic's AI model, so it needs a connection. Everything else in FIRESight works without it.
      </p>
      <ul className="mt-5 space-y-3.5">
        <Way title="Use your own API key">
          <p>Create a key in the Anthropic Console and paste it into Settings. The key is kept only in this browser's local storage and is sent only to Anthropic, directly from this browser. Usage is billed to your Anthropic account.</p>
          <div className="flex flex-wrap gap-2 pt-0.5">
            <Button size="sm" variant="primary" icon="settings" onClick={onOpenSettings}>
              Open Settings
            </Button>
            <a
              href={CONSOLE_KEYS_URL}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex h-7 items-center gap-1.5 rounded-sm border border-line-strong bg-surface-2 px-2 text-xs font-medium text-ink-1 hover:bg-surface-3"
            >
              Create a key at Anthropic <Icon name="arrowRight" size={12} className="-rotate-45" />
            </a>
          </div>
        </Way>
        <Way title="Hosted by this site">
          {checking ? (
            <Skeleton className="h-4 w-2/3" />
          ) : (
            <p>The operator of a FIRESight site can provide the connection, so visitors need no key. This site has not set that up.</p>
          )}
        </Way>
      </ul>
      <p className="mt-5 flex items-start gap-2 border-t border-line pt-3.5 text-xs leading-relaxed text-ink-3">
        <Icon name="lock" size={12} className="mt-[3px]" />
        <span>
          Simulation always runs on your device. When you use the assistant, your messages, the plan, any files you attach and the short results of the runs it asks for are sent to Claude to produce the answer. Nothing is sent until you send a message.
        </span>
      </p>
    </div>
  );
}

interface WelcomeProps {
  emptyPlan: boolean;
  onAsk: (text: string) => void;
  onReview: () => void;
}

const QUESTIONS = ['How can we retire earlier?', 'What are the biggest risks to this plan?', 'How much could we spend and still be fine in 90 of 100 lives?', EQUITY_PROMPT];

/** Shown when connected and nothing has been said yet. */
export function Welcome({ emptyPlan, onAsk, onReview }: WelcomeProps) {
  if (emptyPlan) {
    return (
      <div>
        <h3 className="text-lg font-semibold tracking-tight text-balance">Describe your situation and review the plan it becomes</h3>
        <p className="mt-2 text-sm leading-relaxed text-ink-2">
          Write the way you would tell a friend: who is in the household and how old, where you live, what you earn and save, what you spend, what you own and owe, and when you hope to stop working. Rough figures are fine.
        </p>
        <ul className="mt-4 space-y-2.5 border-t border-line pt-3.5 text-sm leading-relaxed text-ink-2">
          <li>
            <span className="font-medium text-ink-1">Attach documents if you like.</span> Pay stubs, account, mortgage and Social Security statements, as images or PDFs.
          </li>
          <li>
            <span className="font-medium text-ink-1">You approve every entry.</span> Each part of the plan arrives as a card showing what would be set and which values were assumed rather than stated.
          </li>
          <li>
            <span className="font-medium text-ink-1">Nothing is simulated until you confirm</span> the summary of what was understood.
          </li>
        </ul>
      </div>
    );
  }
  return (
    <div>
      <h3 className="text-lg font-semibold tracking-tight text-balance">Ask about this plan</h3>
      <p className="mt-2 text-sm leading-relaxed text-ink-2">
        Every figure in an answer comes from a simulation run on your device, and each answer lists the runs behind it. What-ifs are tested on the same simulated futures as your plan, so differences are real and not luck.
      </p>
      <div className="mt-4 border-t border-line pt-3.5">
        <p className="eyebrow">Start with</p>
        <ul className="mt-2 flex flex-col items-start gap-1.5">
          <li>
            <button type="button" onClick={onReview} className="flex items-center gap-2 rounded-sm border border-line bg-surface-2 px-2.5 py-1.5 text-left text-xs font-medium text-ink-1 hover:border-line-strong">
              <Icon name="checkCircle" size={13} className="text-accent" /> Review my plan
            </button>
          </li>
          {QUESTIONS.map((q) => (
            <li key={q}>
              <button type="button" onClick={() => onAsk(q)} className="rounded-sm border border-line bg-surface-2 px-2.5 py-1.5 text-left text-xs leading-snug text-ink-2 hover:border-line-strong hover:text-ink-1">
                {q}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
