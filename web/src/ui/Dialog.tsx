import { useEffect, useId, useRef, type ReactNode } from 'react';
import { cx } from './cx';
import { IconButton } from './Button';

interface ModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  className?: string;
}

// Both are native <dialog> elements shown modally: the browser provides the focus
// trap, Escape handling, the top layer and inertness of the page behind.
function useModal(open: boolean, onClose: () => void) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (open && !el.open) el.showModal();
    if (!open && el.open) el.close();
  }, [open]);
  const handlers = {
    onClose,
    onCancel: (e: React.SyntheticEvent) => {
      e.preventDefault();
      onClose();
    },
    // A press on the backdrop lands on the <dialog> element itself.
    onPointerDown: (e: React.PointerEvent<HTMLDialogElement>) => {
      if (e.target === e.currentTarget) onClose();
    },
  };
  return { ref, handlers };
}

function Header({ id, title, description, onClose }: { id: string; title: string; description?: ReactNode; onClose: () => void }) {
  return (
    <header className="flex items-start gap-3 border-b border-line px-5 py-4">
      <div className="min-w-0 flex-1">
        <h2 id={id} className="text-lg font-semibold tracking-tight text-ink-1">
          {title}
        </h2>
        {description && <p className="mt-0.5 text-sm text-ink-2">{description}</p>}
      </div>
      <IconButton icon="x" label="Close" size="sm" onClick={onClose} className="-mr-1.5 -mt-0.5" />
    </header>
  );
}

/** A centred modal for a focused task. */
export function Dialog({ open, onClose, title, description, children, footer, className }: ModalProps) {
  const { ref, handlers } = useModal(open, onClose);
  const id = useId();
  return (
    <dialog
      ref={ref}
      aria-labelledby={id}
      {...handlers}
      className={cx(
        'm-auto w-[min(34rem,calc(100vw-2rem))] max-h-[min(44rem,calc(100dvh-2rem))] overflow-hidden rounded-lg border border-line bg-surface-1 p-0 text-ink-1 shadow-overlay',
        'backdrop:bg-scrim open:flex open:flex-col open:animate-pop-in',
        className,
      )}
    >
      {open && (
        <>
          <Header id={id} title={title} description={description} onClose={onClose} />
          <div className="scroll-thin min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>
          {footer && <footer className="flex items-center justify-end gap-2 border-t border-line px-5 py-3">{footer}</footer>}
        </>
      )}
    </dialog>
  );
}

interface SheetProps extends Omit<ModalProps, 'description' | 'footer'> {
  side?: 'left' | 'right' | 'bottom';
  /** Hides the header when the content brings its own. */
  bare?: boolean;
}

const SIDE = {
  left: 'mr-auto ml-0 h-dvh max-h-dvh w-[min(26rem,100vw)] border-r open:animate-slide-in-left',
  right: 'ml-auto mr-0 h-dvh max-h-dvh w-[min(26rem,100vw)] border-l open:animate-slide-in-right',
  bottom: 'mt-auto mb-0 max-h-[88dvh] w-screen max-w-none rounded-t-lg border-t open:animate-slide-in-up',
};

/** A modal panel that slides in from an edge: the editor and assistant on small screens. */
export function Sheet({ open, onClose, title, children, side = 'right', bare, className }: SheetProps) {
  const { ref, handlers } = useModal(open, onClose);
  const id = useId();
  return (
    <dialog
      ref={ref}
      aria-labelledby={bare ? undefined : id}
      aria-label={bare ? title : undefined}
      {...handlers}
      className={cx(
        'max-w-none overflow-hidden border-line bg-surface-1 p-0 text-ink-1 shadow-overlay backdrop:bg-scrim open:flex open:flex-col',
        SIDE[side],
        className,
      )}
    >
      {open && (
        <>
          {!bare && <Header id={id} title={title} onClose={onClose} />}
          <div className="scroll-thin min-h-0 flex-1 overflow-y-auto">{children}</div>
        </>
      )}
    </dialog>
  );
}
