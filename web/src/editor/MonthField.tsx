import { set } from '@/state/patches';
import { Field, NumberInput, Select, currentYearMonth } from '@/ui';
import { useEdit, useHelp, useIssues, useValue } from './hooks';

const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'].map(
  (label, i) => ({ value: String(i + 1).padStart(2, '0'), label }),
);

interface MonthInputProps {
  value: string;
  onChange: (value: string) => void;
  id?: string;
  invalid?: boolean;
  label: string;
}

/** Month and year as two controls: works the same in every browser, unlike type="month". */
export function MonthInput({ value, onChange, id, invalid, label }: MonthInputProps) {
  const [year = '', month = '01'] = (value || currentYearMonth()).split('-');
  return (
    <span className="grid grid-cols-[1fr_5.5rem] gap-1.5">
      <Select id={id} aria-label={`${label}: month`} invalid={invalid} value={month} options={MONTHS} onChange={(m) => onChange(`${year}-${m}`)} />
      <NumberInput
        aria-label={`${label}: year`}
        kind="integer"
        invalid={invalid}
        value={Number(year)}
        min={1900}
        max={2200}
        onChange={(y) => y !== null && y >= 1000 && y <= 9999 && onChange(`${y}-${month}`)}
      />
    </span>
  );
}

/** A "YYYY-MM" profile field. With `nullable`, a switch turns the value on and off. */
export function MonthField({ path, label, hint, fallback }: { path: string; label: string; hint?: string; fallback?: string }) {
  const value = useValue<string | null>(path);
  const edit = useEdit();
  const help = useHelp(path);
  const issues = useIssues(path);
  return (
    <Field label={label} help={help} hint={hint} issues={issues}>
      {({ id, invalid }) => (
        <span data-path={path} className="block">
          <MonthInput id={id} label={label} invalid={invalid} value={value ?? fallback ?? currentYearMonth()} onChange={(v) => edit(set(path, v), { coalesce: path })} />
        </span>
      )}
    </Field>
  );
}
