// Editor fields bound to a patch path: each reads its value from the store, writes
// through `edit()` as a `set` operation, shows the schema description as help and the
// engine's issues for that path.
import type { ReactNode } from 'react';
import { join } from '@/state/pointer';
import { set } from '@/state/patches';
import {
  Field,
  NumberInput,
  SegmentedControl,
  Select,
  Slider,
  Switch,
  TextInput,
  cx,
  type NumberKind,
  type Segment,
  type SelectOption,
} from '@/ui';
import { useEdit, useHelp, useIssues, usePeople, useValue } from './hooks';

interface Common {
  path: string;
  label: string;
  /** Overrides the schema description. */
  help?: ReactNode;
  hint?: ReactNode;
  className?: string;
}

function useBinding<T>(path: string, help?: ReactNode) {
  const value = useValue<T>(path);
  const edit = useEdit();
  const schemaHelp = useHelp(path);
  const issues = useIssues(path);
  const write = (next: unknown, label?: string) => edit(set(path, next), { coalesce: path, label });
  return { value, write, help: help ?? schemaHelp, issues };
}

interface NumProps extends Common {
  kind?: NumberKind;
  nullable?: boolean;
  placeholder?: string;
  suffix?: ReactNode;
  min?: number;
  max?: number;
  step?: number;
}

export function NumField({ path, label, help, hint, className, kind = 'number', ...rest }: NumProps) {
  const b = useBinding<number | null>(path, help);
  return (
    <Field label={label} help={b.help} hint={hint} issues={b.issues} className={className}>
      {({ id, describedBy, invalid }) => (
        <span data-path={path} className="block">
          <NumberInput id={id} aria-describedby={describedBy} invalid={invalid} kind={kind} value={b.value ?? null} onChange={(v) => b.write(v)} {...rest} />
        </span>
      )}
    </Field>
  );
}

export const MoneyField = (props: Omit<NumProps, 'kind'>) => <NumField {...props} kind="money" />;
export const PercentField = (props: Omit<NumProps, 'kind'>) => <NumField {...props} kind="percent" />;

interface SliderFieldProps extends Common {
  kind?: NumberKind;
  min: number;
  max: number;
  step: number;
  suffix?: ReactNode;
}

export function SliderField({ path, label, help, hint, className, ...rest }: SliderFieldProps) {
  const b = useBinding<number>(path, help);
  return (
    <Field label={label} help={b.help} hint={hint} issues={b.issues} className={className}>
      {({ id, invalid }) => (
        <span data-path={path} className="block">
          <Slider id={id} aria-label={label} invalid={invalid} value={b.value ?? 0} onChange={(v) => b.write(v)} {...rest} />
        </span>
      )}
    </Field>
  );
}

export function TextField({ path, label, help, hint, className, placeholder }: Common & { placeholder?: string }) {
  const b = useBinding<string | null>(path, help);
  return (
    <Field label={label} help={b.help} hint={hint} issues={b.issues} className={className}>
      {({ id, describedBy, invalid }) => (
        <span data-path={path} className="block">
          <TextInput id={id} aria-describedby={describedBy} invalid={invalid} value={b.value ?? ''} placeholder={placeholder} onChange={(v) => b.write(v)} />
        </span>
      )}
    </Field>
  );
}

interface ChoiceProps<T extends string> extends Common {
  options: readonly SelectOption<T>[];
  /** Maps the stored value to an option value and back, for non-string fields. */
  read?: (stored: unknown) => T;
  write?: (value: T) => unknown;
}

export function ChoiceField<T extends string>({ path, label, help, hint, className, options, read, write }: ChoiceProps<T>) {
  const b = useBinding<unknown>(path, help);
  const value = read ? read(b.value) : (b.value as T);
  return (
    <Field label={label} help={b.help} hint={hint} issues={b.issues} className={className}>
      {({ id, describedBy, invalid }) => (
        <span data-path={path} className="block">
          <Select id={id} aria-describedby={describedBy} invalid={invalid} value={value} options={options} onChange={(v) => b.write(write ? write(v) : v)} />
        </span>
      )}
    </Field>
  );
}

export function SegmentField<T extends string>({
  path,
  label,
  help,
  hint,
  className,
  options,
  read,
  write,
}: Common & { options: readonly Segment<T>[]; read?: (stored: unknown) => T; write?: (value: T) => unknown }) {
  const b = useBinding<unknown>(path, help);
  const value = read ? read(b.value) : (b.value as T);
  return (
    <Field label={label} help={b.help} hint={hint} issues={b.issues} className={className}>
      {({ id }) => (
        <span data-path={path} className="block">
          <SegmentedControl id={id} aria-label={label} fill value={value} options={options} onChange={(v) => b.write(write ? write(v) : v)} />
        </span>
      )}
    </Field>
  );
}

export function ToggleField({ path, label, help, hint, className }: Common) {
  const b = useBinding<boolean>(path, help);
  return (
    <Field inline label={label} help={b.help} hint={hint} issues={b.issues} className={className}>
      {({ id, describedBy }) => (
        <span data-path={path} className="flex">
          <Switch id={id} aria-describedby={describedBy} checked={Boolean(b.value)} onChange={(v) => b.write(v)} />
        </span>
      )}
    </Field>
  );
}

/** Which adult a thing belongs to. Hidden when the household has one person. */
export function OwnerField({ path, label = 'Owner', nullable, className }: { path: string; label?: string; nullable?: boolean; className?: string }) {
  const people = usePeople();
  if (people.length < 2 && !nullable) return null;
  const options = nullable ? [{ value: 'shared', label: 'Shared' }, ...people] : people;
  return (
    <ChoiceField
      path={path}
      label={label}
      className={className}
      options={options}
      read={(v) => (v === null || v === undefined ? 'shared' : String(v))}
      write={(v) => (v === 'shared' ? null : Number(v))}
    />
  );
}

/** Two or three fields side by side. */
export function Row({ children, cols = 2, className }: { children: ReactNode; cols?: 2 | 3; className?: string }) {
  return <div className={cx('grid items-start gap-x-3 gap-y-3', cols === 3 ? 'grid-cols-3' : 'grid-cols-2', className)}>{children}</div>;
}

/** A titled group of fields inside a section or item. */
export function Group({ title, children, action, className }: { title?: ReactNode; children: ReactNode; action?: ReactNode; className?: string }) {
  return (
    <div className={cx('space-y-3', className)}>
      {(title || action) && (
        <div className="flex h-5 items-center justify-between">
          <h4 className="eyebrow">{title}</h4>
          {action}
        </div>
      )}
      {children}
    </div>
  );
}

export { join };
