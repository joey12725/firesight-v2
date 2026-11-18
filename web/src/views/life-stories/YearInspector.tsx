// One year of one life, opened up: every line the simulation recorded (money in beside
// money out, which always balance), what moved between accounts, the year-end balance
// sheet, and the tax return for that year as a 1040-style breakdown.
import { useMemo } from 'react';
import { Disclosure, IconButton, cx, money } from '@/ui';
import { STATUS_LABEL, useChartTheme } from '@/ui/charts';
import { taxReturn, yearLedger, type LedgerLine, type LedgerSection, type Life } from './life';

const dollars = (v: number) => money(Math.round(v));

function Lines({ lines, strongLast }: { lines: LedgerLine[]; strongLast?: boolean }) {
  return (
    <>
      {lines.map((l, i) => (
        <div key={l.key} className={cx('flex items-baseline justify-between gap-4 py-1', strongLast && i === lines.length - 1 && 'mt-1 border-t border-line pt-2 font-medium text-ink-1')}>
          <dt className={strongLast && i === lines.length - 1 ? '' : 'text-ink-2'}>{l.label}</dt>
          <dd className="num shrink-0 text-ink-1">{dollars(l.amount)}</dd>
        </div>
      ))}
    </>
  );
}

function Side({ title, sections, total }: { title: string; sections: LedgerSection[]; total: number }) {
  return (
    <section className="min-w-0">
      <h4 className="eyebrow">{title}</h4>
      <dl className="mt-2 text-sm">
        {sections.map((s) => (
          <div key={s.title} className="border-t border-line py-2 first:border-t-0 first:pt-0">
            <div className="flex items-baseline justify-between gap-4 pb-0.5">
              <dt className="font-medium text-ink-1">{s.title}</dt>
              <dd className="num font-medium text-ink-1">{dollars(s.total ?? 0)}</dd>
            </div>
            <div className="pl-3">
              <Lines lines={s.lines} />
            </div>
          </div>
        ))}
        <div className="flex items-baseline justify-between gap-4 border-t border-line-strong pt-2 font-semibold text-ink-1">
          <dt>{title}</dt>
          <dd className="num">{dollars(total)}</dd>
        </div>
      </dl>
    </section>
  );
}

export function YearInspector({ life, year, onChange }: { life: Life; year: number; onChange: (year: number) => void }) {
  const theme = useChartTheme();
  const ledger = useMemo(() => yearLedger(life, year), [life, year]);
  const filed = useMemo(() => taxReturn(life, year), [life, year]);
  const events = life.events.filter((e) => e.year === year);
  const lastYear = life.time.length - 1;
  const partial = ledger.months < 12;

  return (
    <section aria-label={`Ledger for ${ledger.year}`} data-testid="year-inspector">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <div className="flex items-center">
          <IconButton icon="chevronLeft" size="sm" label="Previous year" disabled={year <= 0} onClick={() => onChange(year - 1)} />
          <h3 className="num min-w-[7.5rem] text-center text-base font-semibold tracking-tight text-ink-1">
            {life.hasAges ? `Age ${Math.floor(ledger.age)}` : ledger.year} {life.hasAges && <span className="font-normal text-ink-3">· {ledger.year}</span>}
          </h3>
          <IconButton icon="chevronRight" size="sm" label="Next year" disabled={year >= lastYear} onClick={() => onChange(year + 1)} />
        </div>
        <span className="flex items-center gap-1.5 text-xs text-ink-2">
          <span aria-hidden className="inline-block size-2.5 rounded-[2px]" style={{ background: theme.status[ledger.status] }} />
          {STATUS_LABEL[ledger.status]} at year end
        </span>
        {partial && <span className="text-xs text-ink-3">A part-year: {ledger.months} {ledger.months === 1 ? 'month' : 'months'} recorded.</span>}
      </header>
      {events.length > 0 && (
        <ul className="mt-3 space-y-1 text-sm text-pretty text-ink-1">
          {events.map((e) => (
            <li key={`${e.month}-${e.kind}`} className="flex gap-2">
              <span aria-hidden className="mt-[0.45rem] inline-block size-1.5 shrink-0 rounded-full" style={{ background: e.trouble ? theme.status.depleted : theme.ink.primary }} />
              {e.detail}
            </li>
          ))}
        </ul>
      )}

      <div className="mt-5 grid grid-cols-1 gap-x-10 gap-y-8 @2xl:grid-cols-2 @6xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1.25fr)]">
        <Side title="Money in" sections={ledger.moneyIn} total={ledger.totalIn} />
        <Side title="Money out" sections={ledger.moneyOut} total={ledger.totalOut} />

        <section className="min-w-0 @2xl:col-span-2 @6xl:col-span-1">
          <div className="grid grid-cols-1 gap-x-10 gap-y-8 @2xl:grid-cols-2 @6xl:grid-cols-1">
            <div>
              <h4 className="eyebrow">At the end of the year</h4>
              <dl className="mt-2 text-sm">
                <Lines lines={ledger.balances} strongLast />
              </dl>
            </div>
            {ledger.memo.length > 0 && (
              <div>
                <h4 className="eyebrow">Also that year</h4>
                <dl className="mt-2 text-sm">
                  <Lines lines={ledger.memo} />
                </dl>
              </div>
            )}
          </div>
        </section>
      </div>
      <p className="mt-4 max-w-[48rem] text-xs leading-relaxed text-pretty text-ink-3">
        Every amount is in today’s dollars, exactly as the simulation recorded it. Money in equals money out: whatever arrives is spent, taxed or saved. Cash is the household’s current account, so money drawn from other accounts often shows both as “To cash” and “From cash” on its way through. Tax is shown in the year it was paid, so the April settlement of the
        previous year’s return is in this year’s figure.
      </p>

      {filed && (
        <Disclosure
          className="mt-6 border-t border-line pt-4"
          defaultOpen
          summary={
            <span className="text-sm font-medium text-ink-1">
              The {filed.year} tax return{filed.marginal !== null && <span className="font-normal text-ink-2"> · next dollar of ordinary income taxed at {Math.round(filed.marginal * 100)}% federal</span>}
            </span>
          }
        >
          <div className="scroll-thin mt-3 overflow-x-auto">
            <table className="w-full min-w-[30rem] border-collapse text-sm">
              <caption className="sr-only">
                Tax return for {filed.year}, line by line, as filed and in today’s dollars
              </caption>
              <thead>
                <tr className="border-b border-line text-left text-xs text-ink-3">
                  <th scope="col" className="py-1.5 pr-4 font-normal">
                    Line
                  </th>
                  <th scope="col" className="py-1.5 pl-4 text-right font-normal whitespace-nowrap">
                    As filed, {filed.year} dollars
                  </th>
                  <th scope="col" className="py-1.5 pl-4 text-right font-normal whitespace-nowrap">
                    Today’s dollars
                  </th>
                </tr>
              </thead>
              <tbody>
                {filed.lines.map((l) => (
                  <tr key={l.key} className={cx('border-b border-line align-baseline', l.strong && 'bg-surface-1')}>
                    <th scope="row" className="py-1.5 pr-4 text-left font-normal">
                      <span className={l.strong ? 'font-medium text-ink-1' : 'text-ink-1'}>{l.label}</span>
                      {l.note && <span className="block text-xs leading-snug text-pretty text-ink-3">{l.note}</span>}
                    </th>
                    <td className={cx('num py-1.5 pl-4 text-right whitespace-nowrap text-ink-1', l.strong && 'font-medium')}>{dollars(l.nominal)}</td>
                    <td className="num py-1.5 pl-4 text-right whitespace-nowrap text-ink-2">{dollars(l.real)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="mt-3 max-w-[48rem] text-xs leading-relaxed text-pretty text-ink-3">
            A return is filed in the dollars of its own year: by {filed.year} prices in this life are {filed.priceLevel.toFixed(2)} times today’s, and the right-hand column divides by that. The return covers income earned in{' '}
            {filed.year}; the ledger above shows tax in the year the cash left.
          </p>
        </Disclosure>
      )}
    </section>
  );
}
