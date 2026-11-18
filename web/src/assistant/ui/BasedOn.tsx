import { Disclosure, count } from '@/ui';
import type { ToolPart } from '../conversation';

/** The audit strip under an answer: every tool call behind it, with paths and seed. */
export function BasedOn({ tools }: { tools: ToolPart[] }) {
  const used = tools.filter((t) => t.status === 'done' || t.status === 'error');
  if (used.length === 0) return null;
  const runs = used.reduce((n, t) => n + (t.meta?.runs ?? 0), 0);
  const summary = `Based on ${used.length} step${used.length === 1 ? '' : 's'}${runs > 0 ? `, ${count(runs)} simulation run${runs === 1 ? '' : 's'}` : ''}`;
  return (
    <Disclosure summary={summary} className="mt-3">
      <ol className="space-y-1.5 border-l border-line pl-3 text-xs text-ink-2">
        {used.map((t) => (
          <li key={t.id} className="leading-snug">
            <span className={t.status === 'error' ? 'text-warn' : undefined}>{t.title}</span>
            {t.meta && (t.meta.paths !== undefined || t.meta.seed !== undefined) && (
              <span className="num block text-2xs text-ink-3">
                {[
                  t.meta.paths !== undefined && t.meta.paths > 1 ? `${count(t.meta.paths)} lives${(t.meta.runs ?? 1) > 1 ? ' per run' : ''}` : null,
                  t.meta.seed !== undefined ? `seed ${t.meta.seed}` : null,
                  t.meta.paired ? 'paired on identical futures' : null,
                ]
                  .filter(Boolean)
                  .join(' · ')}
              </span>
            )}
          </li>
        ))}
      </ol>
      <p className="mt-2 text-2xs leading-snug text-ink-3">Figures in the answer come from these results. Open a step above to see exactly what was asked and returned.</p>
    </Disclosure>
  );
}
