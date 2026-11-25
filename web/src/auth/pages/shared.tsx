// Pieces the account pages share: the page frame, a copyable code block, small helpers.
import type { ReactNode } from 'react';
import { navigate } from '@/state/route';
import { toast } from '@/state/ui-store';
import { Icon, IconButton, cx } from '@/ui';

/** A scrolling page under the top bar, with a way back to the app. */
export function PageFrame({ width = 'narrow', children }: { width?: 'narrow' | 'wide'; children: ReactNode }) {
  return (
    <main className="scroll-thin h-full overflow-y-auto bg-surface-0">
      <div className={cx('mx-auto px-4 pb-20 pt-8 sm:px-8 sm:pt-12', width === 'narrow' ? 'max-w-md' : 'max-w-3xl')}>
        <button type="button" onClick={() => navigate('/')} className="mb-6 inline-flex items-center gap-1 rounded-xs text-xs font-medium text-ink-2 hover:text-accent-ink">
          <Icon name="chevronLeft" size={13} />
          Back to FIRESight
        </button>
        {children}
      </div>
    </main>
  );
}

export async function copyText(text: string, what = 'Copied') {
  try {
    await navigator.clipboard.writeText(text);
    toast({ tone: 'good', title: what });
  } catch {
    toast({ tone: 'warn', title: 'Select the text and copy it by hand', description: 'This browser did not allow access to the clipboard.' });
  }
}

/** Monospace text with a copy button: a command, a config snippet, an address. */
export function CodeBlock({ text, label, className }: { text: string; label: string; className?: string }) {
  return (
    <div className={cx('flex items-start gap-1 rounded-sm border border-line bg-surface-2 py-1 pl-2.5 pr-1', className)}>
      <pre className="num scroll-thin min-w-0 flex-1 overflow-x-auto py-1.5 text-xs leading-relaxed text-ink-1">{text}</pre>
      <IconButton icon="copy" size="sm" label={`Copy ${label}`} onClick={() => void copyText(text)} />
    </div>
  );
}

export function ErrorNote({ children }: { children: ReactNode }) {
  if (!children) return null;
  return (
    <p role="alert" className="flex items-start gap-1.5 text-sm leading-snug text-bad">
      <Icon name="alertCircle" size={14} className="mt-[3px]" />
      <span>{children}</span>
    </p>
  );
}

export const shortDate = (value: string | number | Date | null | undefined): string => {
  if (value === null || value === undefined) return 'never';
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? 'never' : date.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
};

/** What an OAuth scope lets an app do, in plain words. Unknown scopes are shown as they are. */
export function scopeMeaning(scope: string): string | null {
  switch (scope) {
    case 'plans:read':
      return 'Read the plans synced to your account and run simulations on them';
    case 'plans:write':
      return 'Create plans in your account and change the ones synced to it';
    case 'offline_access':
      return 'Stay connected until you disconnect it, without asking you to sign in each time';
    case 'openid':
      return null;
    case 'profile':
    case 'email':
      return 'See your name and email address';
    default:
      return `"${scope}"`;
  }
}

export const scopeMeanings = (scopes: string[]): string[] => [...new Set(scopes.map(scopeMeaning).filter((s): s is string => s !== null))];
