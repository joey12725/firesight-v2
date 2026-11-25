import { useState } from 'react';
import type { EquityPreview } from '@/engine/equity-types';
import type { OutcomeRow, PrivateCompany } from '@/engine/types';
import { remove, set } from '@/state/patches';
import { Button, Field, IconButton, NumberInput, Popover } from '@/ui';
import { Row, SegmentField } from '../fields';
import { useEdit, useHelp, useIssues } from '../hooks';
import { OUTCOME_LABEL, OUTLOOKS, stageLabel } from '../labels';
import { addOutcome, countEntry, datedRows, futuresByKind, futuresDecimals, futuresText, isNormal, normaliseTable, remainder, removeOutcome, roomFor, toFutures } from './futures';
import { LIMITS, newOutcomeRow, seedCustomTable } from './model';
import { kindHeading, multipleHint, outcomeDetail } from './text';

type Kind = OutcomeRow['kind'];
type DatedKind = Exclude<Kind, 'still_private'>;
const DATED: DatedKind[] = ['fails', 'acquired', 'ipo'];
const ADD_NOTE: Record<DatedKind, string> = {
  fails: 'Another window in which it could shut down',
  acquired: 'A sale at a different time or price',
  ipo: 'A listing at a different time or price',
};

const round1 = (v: number) => Math.round(v * 10) / 10;

/** The stage's typical odds, read-only, with the lean on them. */
function TypicalOdds({ path, company, preview }: { path: string; company: PrivateCompany; preview: EquityPreview }) {
  const edit = useEdit();
  const probabilities = preview.table.map((r) => r.probability);
  const decimals = futuresDecimals(probabilities);
  const futures = toFutures(probabilities, decimals);
  const zero = Math.round(preview.acquisition_zero_share * 100);
  return (
    <>
      <p className="text-xs font-medium text-ink-2">Out of 100 futures for a {stageLabel(company.stage)} company</p>
      <ul className="divide-y divide-line rounded-md border border-line">
        {preview.table.map((row, i) => (
          <li key={row.id} className="px-2.5 py-2">
            <div className="flex items-baseline justify-between gap-3">
              <span className="text-sm text-ink-1">{OUTCOME_LABEL[row.kind]}</span>
              <span className="num shrink-0 text-sm text-ink-1">{futuresText(futures[i] ?? 0, decimals)} of 100</span>
            </div>
            <p className="mt-0.5 text-xs leading-snug text-ink-3">{outcomeDetail(row, company.preferred_price, preview.preference_overhang)}</p>
          </li>
        ))}
      </ul>
      <SegmentField
        path={`${path}/outlook`}
        label="Lean"
        options={OUTLOOKS}
        help="Cautious makes shutting down 10 of 100 futures more likely than typical, with the other outcomes scaled down to make room. Hopeful makes it up to 10 less likely. It has no effect once you use your own odds."
      />
      <p className="text-xs leading-relaxed text-ink-3">
        Years are typical ranges: one in ten of each outcome comes sooner and one in ten later. In a sale below the last valuation investors are paid first: common stock gets nothing in {zero} of 100 sales.
      </p>
      <Button size="sm" variant="secondary" onClick={() => edit(set(`${path}/outcomes`, seedCustomTable(preview.table)), { label: 'Use my own odds' })}>
        Use my own odds
      </Button>
    </>
  );
}

type PairEnd = 'low' | 'high';

/**
 * The two ends of a range (earliest and latest year, low and high price) as one unit:
 * an entry is written when the field is left, and when it crosses the other end that
 * end moves with it in the same edit, so the pair is never out of order.
 */
function RangePair({
  rowPath,
  row,
  fields,
  labels,
  rowName,
  kind,
  step,
  min,
  max,
  suffix,
}: {
  rowPath: string;
  row: OutcomeRow;
  fields: { low: 'years_from' | 'multiple_low'; high: 'years_to' | 'multiple_high' };
  labels: { low: string; high: string };
  rowName: string;
  kind: 'years' | 'number';
  step: number;
  min: number;
  max?: number;
  suffix: string;
}) {
  const edit = useEdit();
  const [moved, setMoved] = useState<PairEnd | null>(null);
  const paths = { low: `${rowPath}/${fields.low}`, high: `${rowPath}/${fields.high}` };
  const help = { low: useHelp(paths.low), high: useHelp(paths.high) };
  const issues = { low: useIssues(paths.low), high: useIssues(paths.high) };
  const values = { low: row[fields.low], high: row[fields.high] };

  const write = (end: PairEnd, value: number | null) => {
    if (value === null) return;
    const other: PairEnd = end === 'low' ? 'high' : 'low';
    const crosses = end === 'low' ? value > values.high : value < values.low;
    const ops = [set(paths[end], value), ...(crosses ? [set(paths[other], value)] : [])];
    if (edit(ops, { coalesce: paths[end] })) setMoved(crosses ? other : null);
  };
  const note = moved ? `${labels[moved]} was moved to match, so the range stays in order.` : undefined;

  return (
    <Row>
      {(['low', 'high'] as const).map((end) => (
        <Field key={end} label={labels[end]} help={help[end]} issues={issues[end]} hint={moved === end ? note : undefined}>
          {({ id, describedBy, invalid }) => (
            <span data-path={paths[end]} className="block" onFocus={() => setMoved(null)}>
              <NumberInput
                id={id}
                aria-label={`${rowName}: ${labels[end].toLowerCase()}`}
                aria-describedby={describedBy}
                invalid={invalid}
                commit="blur"
                kind={kind}
                step={step}
                min={min}
                max={max}
                suffix={suffix}
                value={values[end]}
                onChange={(v) => write(end, v)}
              />
            </span>
          )}
        </Field>
      ))}
    </Row>
  );
}

/** One dated row of a custom table: its count of 100, when, and at what price. */
function CustomRow({
  path,
  company,
  row,
  rows,
  preview,
  write,
}: {
  path: string;
  company: PrivateCompany;
  row: OutcomeRow;
  rows: OutcomeRow[];
  preview: EquityPreview | null;
  write: (next: OutcomeRow[], label: string) => void;
}) {
  // Why the last finished entry was refused. It stays after the field is left, until the field is entered again or the table changes.
  const [refused, setRefused] = useState<{ message: string; shown: number; room: number } | null>(null);
  const rowPath = `${path}/outcomes/@${row.id}`;
  const issues = useIssues(`${rowPath}/probability`);
  const room = round1(roomFor(rows, row.id));
  const shown = round1(row.probability * 100);
  const label = OUTCOME_LABEL[row.kind];
  const overhang = preview?.preference_overhang ?? 0;
  const peers = datedRows(rows).filter((r) => r.kind === row.kind);
  const sameKind = peers.length;
  // A screen reader hears the row with each of its fields ("Is acquired 2: earliest").
  const rowName = sameKind > 1 ? `${label} ${peers.findIndex((r) => r.id === row.id) + 1}` : label;
  // The engine's share of sales that pay common stock nothing covers every sale row together, so it is quoted only when there is one.
  const zero = row.kind === 'acquired' && sameKind === 1 && preview && row.probability > 0 ? Math.round(preview.acquisition_zero_share * 100) : null;
  const message = refused && refused.shown === shown && refused.room === room ? refused.message : null;

  // The field commits when it is left, so this is a finished entry: "7" on the way to "70" never arrives.
  const onCount = (value: number | null) => {
    if (value === null) return;
    const entry = countEntry(rows, row.id, value);
    setRefused(entry.refused ? { message: entry.refused, shown, room } : null);
    if (entry.rows !== rows) write(entry.rows, `${label}: ${futuresText(round1(value), 1)} of 100`);
  };

  return (
    <li className="space-y-3 rounded-md border border-line px-2.5 py-2.5">
      <div className="flex items-start gap-2">
        <Field
          className="flex-1"
          label={`${label}, futures of 100`}
          hint={`Up to ${futuresText(room, 1)}: what this row has plus what is still private.`}
          issues={message ? [{ severity: 'error', message }] : issues}
        >
          {({ id, describedBy }) => (
            <span data-path={`${rowPath}/probability`} className="block" onFocus={() => setRefused(null)}>
              <NumberInput
                id={id}
                aria-label={sameKind > 1 ? `${rowName}, futures of 100` : undefined}
                aria-describedby={describedBy}
                invalid={message !== null}
                commit="blur"
                kind="number"
                step={0.5}
                min={0}
                max={room}
                suffix="of 100"
                value={shown}
                onChange={onCount}
              />
            </span>
          )}
        </Field>
        <IconButton icon="trash" label={`Remove this "${label.toLowerCase()}" row`} variant="danger" className="mt-5" onClick={() => write(removeOutcome(rows, row.id), `Remove an outcome`)} />
      </div>
      <RangePair rowPath={rowPath} row={row} fields={{ low: 'years_from', high: 'years_to' }} labels={{ low: 'Earliest', high: 'Latest' }} rowName={rowName} kind="years" step={0.5} min={0} max={30} suffix="yrs" />
      {row.kind !== 'fails' && (
        <div className="space-y-1">
          <p className="text-xs font-medium text-ink-2">{row.kind === 'ipo' ? 'IPO price, as a multiple of the last round' : 'Sale price of the company, as a multiple of its last valuation'}</p>
          <RangePair rowPath={rowPath} row={row} fields={{ low: 'multiple_low', high: 'multiple_high' }} labels={{ low: 'Low price', high: 'High price' }} rowName={rowName} kind="number" step={0.1} min={0.001} suffix="x" />
          <p className="text-xs leading-snug text-ink-3">{multipleHint(row, company.preferred_price, overhang, zero)}</p>
        </div>
      )}
    </li>
  );
}

/** The household's own table: rows grouped by what happens, with the remainder explicit. */
function CustomOdds({ path, company, preview }: { path: string; company: PrivateCompany; preview: EquityPreview | null }) {
  const edit = useEdit();
  const tableIssues = useIssues(`${path}/outcomes`);
  // Shown in the editor's form (one "still private" row holding the remainder); a table that arrives otherwise is rewritten that way on the first edit.
  const rows = isNormal(company.outcomes) ? company.outcomes : normaliseTable(company.outcomes);
  const write = (next: OutcomeRow[], label: string) => edit(set(`${path}/outcomes`, next), { coalesce: `${path}/outcomes`, label });
  const decimals = futuresDecimals(rows.map((r) => r.probability)) || (rows.some((r) => Math.abs(r.probability * 100 - Math.round(r.probability * 100)) > 1e-6) ? 1 : 0);
  const yours = futuresByKind(rows);
  const typical = preview ? futuresByKind(preview.typical) : null;
  const full = rows.length >= LIMITS.outcomes;
  const optimistic = tableIssues.find((i) => i.severity === 'warning');
  const left = remainder(rows);

  return (
    <>
      <p className="text-xs font-medium text-ink-2">Your odds, out of 100 futures</p>
      {DATED.map((kind) => {
        const mine = datedRows(rows).filter((r) => r.kind === kind);
        if (mine.length === 0) return null;
        return (
          <div key={kind} className="space-y-1.5">
            <p className="text-xs font-medium text-ink-2">{kindHeading(OUTCOME_LABEL[kind], yours[kind], typical ? typical[kind] : null, decimals)}</p>
            <ul className="space-y-1.5">
              {mine.map((row) => (
                <CustomRow key={row.id} path={path} company={company} row={row} rows={rows} preview={preview} write={write} />
              ))}
            </ul>
          </div>
        );
      })}
      <div data-path={`${path}/outcomes`} className="rounded-md border border-line px-2.5 py-2">
        <div className="flex items-baseline justify-between gap-3">
          <span className="text-sm text-ink-1">{OUTCOME_LABEL.still_private}</span>
          <span data-testid="still-private-count" className="num shrink-0 text-sm text-ink-1">
            {futuresText(left, 1)} of 100
          </span>
        </div>
        <p className="mt-0.5 text-xs leading-snug text-ink-3">
          Always 100 less the rows above{typical ? ` (${futuresText(typical.still_private, decimals)} typical)` : ''}. The same odds then apply to the next ten years. After thirty the equity is counted as worth nothing.
        </p>
      </div>
      {tableIssues
        .filter((i) => i.severity === 'error')
        .map((i) => (
          <p key={i.message} className="text-xs leading-snug text-bad">
            {i.message}
          </p>
        ))}
      <Popover
        label="Add an outcome"
        className="w-72 p-1"
        trigger={
          <Button size="sm" variant="ghost" icon="plus" className="-ml-2" disabled={full}>
            Add an outcome
          </Button>
        }
      >
        {(close) => (
          <ul>
            {DATED.map((kind) => (
              <li key={kind}>
                <button
                  type="button"
                  onClick={() => {
                    write(addOutcome(rows, newOutcomeRow(kind, rows, preview?.typical ?? [])), `Add an outcome: ${OUTCOME_LABEL[kind].toLowerCase()}`);
                    close();
                  }}
                  className="block w-full rounded-sm px-2.5 py-1.5 text-left hover:bg-surface-3"
                >
                  <span className="block text-sm font-medium text-ink-1">{OUTCOME_LABEL[kind]}</span>
                  <span className="block text-xs text-ink-3">{ADD_NOTE[kind]}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </Popover>
      {full && <p className="text-xs text-ink-3">A table can have at most {LIMITS.outcomes} rows.</p>}
      {/* The warning opens with the same sentence, so the plain line is shown only without it. */}
      {preview && !optimistic && (
        <p className="text-xs leading-relaxed text-ink-3">
          These odds put a share at about {preview.implied_multiple.toFixed(1)} times the last round's price; typical odds for this stage put it at {preview.typical_implied_multiple.toFixed(1)}.
        </p>
      )}
      {optimistic && <p className="text-xs leading-relaxed text-warn">{optimistic.message}</p>}
      <Button size="sm" variant="ghost" icon="rotate" className="-ml-2" onClick={() => edit(remove(`${path}/outcomes`), { label: 'Back to typical odds' })}>
        Back to typical odds
      </Button>
    </>
  );
}

/**
 * "What might happen": the stage's typical odds with a lean, or the household's own
 * table. Counts are futures out of 100.
 */
export function OutcomeEditor({ path, company, preview }: { path: string; company: PrivateCompany; preview: EquityPreview | null }) {
  if (company.outcomes.length > 0) return <CustomOdds path={path} company={company} preview={preview} />;
  if (!preview) return <p className="text-xs text-ink-3">The typical odds appear once the errors above are fixed.</p>;
  return <TypicalOdds path={path} company={company} preview={preview} />;
}
