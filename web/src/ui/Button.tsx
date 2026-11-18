import type { ButtonHTMLAttributes, ReactNode, Ref } from 'react';
import { cx } from './cx';
import { Icon, type IconName } from './icons';
import { Tooltip } from './Tooltip';

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger';
type Size = 'sm' | 'md' | 'lg';

const BASE =
  'inline-flex shrink-0 items-center justify-center gap-1.5 rounded-sm font-medium whitespace-nowrap select-none ' +
  'transition-colors duration-[var(--duration-fast)] disabled:opacity-45 disabled:pointer-events-none';

const VARIANT: Record<Variant, string> = {
  primary: 'bg-accent text-on-accent hover:bg-accent-hover',
  secondary: 'border border-line-strong bg-surface-2 text-ink-1 hover:bg-surface-3',
  ghost: 'text-ink-2 hover:bg-surface-3 hover:text-ink-1',
  danger: 'text-bad hover:bg-bad-soft',
};

const SIZE: Record<Size, string> = {
  sm: 'h-7 px-2 text-xs',
  md: 'h-8 px-3 text-sm',
  lg: 'h-10 px-4 text-base',
};

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  icon?: IconName;
  iconRight?: IconName;
  ref?: Ref<HTMLButtonElement>;
}

export function Button({ variant = 'secondary', size = 'md', icon, iconRight, className, children, ...rest }: ButtonProps) {
  return (
    <button type="button" {...rest} className={cx(BASE, VARIANT[variant], SIZE[size], className)}>
      {icon && <Icon name={icon} size={size === 'lg' ? 16 : 14} />}
      {children}
      {iconRight && <Icon name={iconRight} size={size === 'lg' ? 16 : 14} />}
    </button>
  );
}

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  icon: IconName;
  /** Required: the accessible name, also shown as a tooltip. */
  label: string;
  variant?: Variant;
  size?: Size;
  /** Extra content inside the tooltip, such as a keyboard shortcut. */
  hint?: ReactNode;
  pressed?: boolean;
  ref?: Ref<HTMLButtonElement>;
}

const ICON_SIZE: Record<Size, string> = { sm: 'size-7', md: 'size-8', lg: 'size-10' };

export function IconButton({ icon, label, hint, variant = 'ghost', size = 'md', pressed, className, ...rest }: IconButtonProps) {
  return (
    <Tooltip content={hint ? <>{label} <span className="text-ink-3">{hint}</span></> : label}>
      <button
        type="button"
        aria-label={label}
        aria-pressed={pressed}
        {...rest}
        className={cx(BASE, VARIANT[variant], ICON_SIZE[size], pressed && 'bg-surface-3 text-ink-1', className)}
      >
        <Icon name={icon} size={size === 'sm' ? 14 : 16} />
      </button>
    </Tooltip>
  );
}
