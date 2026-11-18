import { useState, type InputHTMLAttributes, type ReactNode, type Ref, type SelectHTMLAttributes } from 'react';
import { cx } from './cx';
import { formatForInput, parseNumber, type NumberKind } from './format';
import { Icon } from './icons';

export const CONTROL =
  'h-8 min-w-0 rounded-sm border bg-surface-2 text-sm text-ink-1 transition-colors duration-[var(--duration-fast)] ' +
  'hover:border-line-strong focus-visible:border-accent focus-visible:outline-offset-0 disabled:opacity-50';

const border = (invalid?: boolean) => (invalid ? 'border-bad' : 'border-line');

export interface TextInputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'onChange' | 'value'> {
  value: string;
  onChange: (value: string) => void;
  invalid?: boolean;
  ref?: Ref<HTMLInputElement>;
}

export function TextInput({ value, onChange, invalid, className, ...rest }: TextInputProps) {
  return (
    <input
      type="text"
      autoComplete="off"
      spellCheck={false}
      {...rest}
      value={value}
      aria-invalid={invalid || undefined}
      onChange={(e) => onChange(e.target.value)}
      className={cx(CONTROL, border(invalid), 'px-2', className ?? 'w-full')}
    />
  );
}

export interface NumberInputProps {
  value: number | null;
  /** Called with a number for valid input, and with null when a nullable field is cleared. */
  onChange: (value: number | null) => void;
  kind?: NumberKind;
  /** Allow an empty field (the profile value becomes null, meaning "use the default"). */
  nullable?: boolean;
  min?: number;
  max?: number;
  /** Arrow-key step, in the field's own unit (dollars, percent points, years). */
  step?: number;
  placeholder?: string;
  /** Unit after the figure, such as "/mo" or "yrs". Money and percent get "$" and "%" themselves. */
  suffix?: ReactNode;
  invalid?: boolean;
  disabled?: boolean;
  id?: string;
  className?: string;
  'aria-label'?: string;
  'aria-describedby'?: string;
}

const DEFAULT_STEP: Record<NumberKind, number> = { money: 100, percent: 0.0025, years: 1, months: 1, number: 1, integer: 1 };

/**
 * A text field for a figure. Accepts shorthand ("85k", "1.2m", "4%"), commits every
 * valid keystroke, and tidies the text on blur. Typing never fights the user: while the
 * field has focus its text is theirs.
 */
export function NumberInput({
  value,
  onChange,
  kind = 'number',
  nullable,
  min,
  max,
  step,
  placeholder,
  suffix,
  invalid,
  disabled,
  id,
  className,
  ...aria
}: NumberInputProps) {
  const shown = value === null || value === undefined ? '' : formatForInput(value, kind);
  // `draft.committed` is the value the text in the field stands for. When the prop
  // differs (undo, the assistant, another field), the draft is replaced.
  const [draft, setDraft] = useState({ text: shown, committed: value });
  const [focused, setFocused] = useState(false);
  const [rejected, setRejected] = useState(false);
  if (draft.committed !== value) {
    setDraft({ text: shown, committed: value });
    setRejected(false);
  }

  const clamp = (n: number) => Math.min(max ?? Infinity, Math.max(min ?? -Infinity, n));

  const commit = (raw: string) => {
    if (raw.trim() === '') {
      setRejected(!nullable);
      setDraft({ text: raw, committed: nullable ? null : draft.committed });
      if (nullable && value !== null) onChange(null);
      return;
    }
    const parsed = parseNumber(raw, kind);
    if (parsed === null) {
      setRejected(true);
      setDraft({ text: raw, committed: draft.committed });
      return;
    }
    setRejected(false);
    setDraft({ text: raw, committed: parsed });
    if (parsed !== value) onChange(parsed);
  };

  const nudge = (direction: 1 | -1, large: boolean) => {
    const s = (step ?? DEFAULT_STEP[kind]) * (large ? 10 : 1);
    const next = Number(clamp(Math.round(((value ?? 0) + direction * s) / s) * s).toPrecision(12));
    setRejected(false);
    setDraft({ text: formatForInput(next, kind), committed: next });
    if (next !== value) onChange(next);
  };

  const prefix = kind === 'money' ? '$' : null;
  const unit = suffix ?? (kind === 'percent' ? '%' : null);

  return (
    <span
      className={cx(
        CONTROL,
        border(invalid || rejected),
        'inline-flex items-center gap-1 px-2 focus-within:border-accent has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-[var(--focus)]',
        disabled && 'opacity-50',
        className ?? 'w-full',
      )}
    >
      {prefix && <span className="num text-ink-3 select-none">{prefix}</span>}
      <input
        id={id}
        type="text"
        inputMode="decimal"
        autoComplete="off"
        spellCheck={false}
        disabled={disabled}
        placeholder={placeholder}
        value={focused ? draft.text : shown}
        aria-invalid={invalid || rejected || undefined}
        {...aria}
        onFocus={(e) => {
          setFocused(true);
          setDraft({ text: shown, committed: value });
          e.target.select();
        }}
        onBlur={() => {
          setFocused(false);
          setRejected(false);
        }}
        onChange={(e) => commit(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
            e.preventDefault();
            nudge(e.key === 'ArrowUp' ? 1 : -1, e.shiftKey);
          } else if (e.key === 'Enter') {
            (e.target as HTMLInputElement).blur();
          }
        }}
        className="num w-full min-w-0 flex-1 bg-transparent text-right text-sm outline-none placeholder:text-ink-3"
      />
      {unit && <span className="shrink-0 text-xs text-ink-3 select-none">{unit}</span>}
    </span>
  );
}

export interface SelectOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
}

export interface SelectProps<T extends string> extends Omit<SelectHTMLAttributes<HTMLSelectElement>, 'onChange' | 'value'> {
  value: T;
  onChange: (value: T) => void;
  options: readonly SelectOption<T>[];
  invalid?: boolean;
  /** Shown as a disabled first option when `value` matches none. */
  placeholder?: string;
}

/** A native select with the app's styling: correct on every platform and input method. */
export function Select<T extends string>({ value, onChange, options, invalid, placeholder, className, ...rest }: SelectProps<T>) {
  const known = options.some((o) => o.value === value);
  return (
    <span className={cx('relative inline-flex min-w-0', className ?? 'w-full')}>
      <select
        {...rest}
        value={known ? value : ''}
        aria-invalid={invalid || undefined}
        onChange={(e) => onChange(e.target.value as T)}
        className={cx(CONTROL, border(invalid), 'w-full appearance-none truncate pl-2 pr-7')}
      >
        {!known && (
          <option value="" disabled>
            {placeholder ?? 'Choose…'}
          </option>
        )}
        {options.map((o) => (
          <option key={o.value} value={o.value} disabled={o.disabled}>
            {o.label}
          </option>
        ))}
      </select>
      <Icon name="chevronDown" size={14} className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-ink-3" />
    </span>
  );
}

export interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  id?: string;
  disabled?: boolean;
  'aria-label'?: string;
  'aria-labelledby'?: string;
  'aria-describedby'?: string;
}

export function Switch({ checked, onChange, disabled, ...rest }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      {...rest}
      onClick={() => onChange(!checked)}
      className={cx(
        'relative inline-flex h-[18px] w-8 shrink-0 items-center rounded-full border transition-colors duration-[var(--duration-base)] disabled:opacity-50',
        checked ? 'border-accent bg-accent' : 'border-line-strong bg-surface-3',
      )}
    >
      <span
        className={cx(
          'size-3 rounded-full transition-transform duration-[var(--duration-base)] ease-out',
          checked ? 'translate-x-[16px] bg-on-accent' : 'translate-x-[3px] bg-ink-3',
        )}
      />
    </button>
  );
}

export interface SliderProps {
  value: number;
  onChange: (value: number) => void;
  min: number;
  max: number;
  step: number;
  kind?: NumberKind;
  suffix?: ReactNode;
  id?: string;
  'aria-label'?: string;
  invalid?: boolean;
}

/** A range slider paired with exact numeric entry. The number may exceed the slider's range. */
export function Slider({ value, onChange, min, max, step, kind = 'number', suffix, id, invalid, ...aria }: SliderProps) {
  const fraction = Math.max(0, Math.min(1, (value - min) / (max - min)));
  return (
    <div className="flex items-center gap-3">
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={Math.max(min, Math.min(max, value))}
        onChange={(e) => onChange(Number(e.target.value))}
        aria-label={aria['aria-label']}
        tabIndex={-1}
        style={{ '--fill': `${fraction * 100}%` } as React.CSSProperties}
        className="slider h-8 w-0 min-w-0 flex-1"
      />
      <NumberInput
        id={id}
        value={value}
        onChange={(v) => v !== null && onChange(v)}
        kind={kind}
        suffix={suffix}
        step={step}
        invalid={invalid}
        className="w-24 shrink-0"
        {...aria}
      />
    </div>
  );
}
