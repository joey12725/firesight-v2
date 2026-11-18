import type { When } from '@/engine/types';
import { set } from '@/state/patches';
import { Field, NumberInput, Select, currentYearMonth } from '@/ui';
import { useEdit, useHelp, useIssues, usePeople, useValue } from './hooks';
import { MonthInput } from './MonthField';

type Kind = When['type'] | 'unset';

const LABELS: Record<Kind, string> = {
  unset: 'Not set',
  start: 'Now',
  date: 'On a date',
  age: 'At an age',
  retirement: 'At retirement',
};

function make(kind: When['type']): When {
  switch (kind) {
    case 'date':
      return { type: 'date', at: currentYearMonth() };
    case 'age':
      return { type: 'age', person: 0, age: 65 };
    case 'retirement':
      return { type: 'retirement', offset_years: 0 };
    case 'start':
      return { type: 'start' };
  }
}

interface WhenFieldProps {
  path: string;
  label: string;
  /** Label for the empty choice when the field is optional, for example "From the start". */
  unsetLabel?: string;
  /** Leave out "Now" where it makes no sense (an end date). */
  kinds?: When['type'][];
}

/** A point in time: a date, someone's age, or relative to retirement. */
export function WhenField({ path, label, unsetLabel, kinds = ['date', 'age', 'retirement'] }: WhenFieldProps) {
  const value = useValue<When | null>(path);
  const edit = useEdit();
  const help = useHelp(path);
  const issues = useIssues(path, true);
  const people = usePeople();
  const write = (next: When | null) => edit(set(path, next), { coalesce: path });

  const kind: Kind = value?.type ?? 'unset';
  const options = [
    ...(unsetLabel !== undefined ? [{ value: 'unset' as Kind, label: unsetLabel }] : []),
    ...kinds.map((k) => ({ value: k as Kind, label: LABELS[k] })),
  ];

  return (
    <Field label={label} help={help} issues={issues}>
      {({ id, invalid }) => (
        <div data-path={path} className="space-y-1.5">
          <Select id={id} invalid={invalid} value={kind} options={options} onChange={(k) => write(k === 'unset' ? null : make(k))} />
          {value?.type === 'date' && <MonthInput label={label} value={value.at} onChange={(at) => write({ type: 'date', at })} />}
          {value?.type === 'age' && (
            <div className="grid grid-cols-2 gap-1.5">
              {people.length > 1 && (
                <Select aria-label={`${label}: whose age`} value={String(value.person)} options={people} onChange={(p) => write({ ...value, person: Number(p) })} />
              )}
              <NumberInput aria-label={`${label}: age`} kind="years" suffix="yrs old" value={value.age} onChange={(age) => age !== null && write({ ...value, age })} />
            </div>
          )}
          {value?.type === 'retirement' && (
            <NumberInput
              aria-label={`${label}: years after retirement`}
              kind="years"
              suffix="yrs after (negative = before)"
              value={value.offset_years}
              onChange={(offset_years) => offset_years !== null && write({ type: 'retirement', offset_years })}
            />
          )}
        </div>
      )}
    </Field>
  );
}

/** "Sep 2031", "age 55", "at retirement": for collapsed summaries. */
export function describeWhen(when: When | null, names: string[]): string {
  if (!when) return '';
  switch (when.type) {
    case 'start':
      return 'now';
    case 'date': {
      const [y, m] = when.at.split('-');
      return `${['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'][Number(m) - 1] ?? ''} ${y}`;
    }
    case 'age':
      return names.length > 1 ? `${names[when.person] ?? 'Person'} at ${when.age}` : `age ${when.age}`;
    case 'retirement':
      return when.offset_years === 0
        ? 'at retirement'
        : `${Math.abs(when.offset_years)} yr ${when.offset_years > 0 ? 'after' : 'before'} retirement`;
  }
}
