import { useEffect } from 'react';
import { useUi, type Toast as ToastModel } from '@/state/ui-store';
import { IconButton } from './Button';
import { cx } from './cx';
import { Icon, type IconName } from './icons';

const TONE: Record<ToastModel['tone'], { icon: IconName; color: string }> = {
  neutral: { icon: 'info', color: 'text-ink-3' },
  good: { icon: 'checkCircle', color: 'text-good' },
  warn: { icon: 'alertTriangle', color: 'text-warn' },
  bad: { icon: 'alertCircle', color: 'text-bad' },
};

function ToastItem({ toast }: { toast: ToastModel }) {
  const dismiss = useUi((s) => s.dismissToast);
  useEffect(() => {
    if (toast.duration <= 0) return;
    const timer = setTimeout(() => dismiss(toast.id), toast.duration);
    return () => clearTimeout(timer);
  }, [toast.id, toast.duration, dismiss]);
  const tone = TONE[toast.tone];
  return (
    <li className="pointer-events-auto flex w-[min(22rem,calc(100vw-2rem))] items-start gap-2.5 rounded-md border border-line bg-surface-2 py-2.5 pl-3 pr-1.5 shadow-overlay animate-pop-in">
      <Icon name={tone.icon} className={cx('mt-0.5', tone.color)} />
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium text-ink-1">{toast.title}</p>
        {toast.description && <p className="mt-0.5 text-xs leading-snug text-ink-2">{toast.description}</p>}
        {toast.action && (
          <button
            type="button"
            className="mt-1.5 text-xs font-medium text-accent-ink hover:underline"
            onClick={() => {
              toast.action?.run();
              dismiss(toast.id);
            }}
          >
            {toast.action.label}
          </button>
        )}
      </div>
      <IconButton icon="x" label="Dismiss" size="sm" onClick={() => dismiss(toast.id)} />
    </li>
  );
}

/** Renders the toast stack. Mount once, near the root. */
export function Toaster() {
  const toasts = useUi((s) => s.toasts);
  return (
    <ol
      aria-live="polite"
      aria-label="Notifications"
      className="pointer-events-none fixed bottom-4 left-1/2 z-[80] flex -translate-x-1/2 flex-col items-center gap-2"
    >
      {toasts.map((t) => (
        <ToastItem key={t.id} toast={t} />
      ))}
    </ol>
  );
}
