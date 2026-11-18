import { cloneElement, useCallback, useEffect, useId, useState, type ReactElement, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { cx } from './cx';
import { assignRef, useFloating, type Placement } from './useFloating';

interface PopoverProps {
  /** The trigger: one button-like element. */
  trigger: ReactElement<Record<string, unknown>>;
  /** Content, or a function that receives `close`. */
  children: ReactNode | ((close: () => void) => ReactNode);
  placement?: Placement;
  /** Accessible name of the panel. */
  label: string;
  className?: string;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}

function Content({ render, close }: { render: PopoverProps['children']; close: () => void }) {
  return <>{typeof render === 'function' ? render(close) : render}</>;
}

/**
 * A non-modal panel anchored to its trigger. Closes on Escape or an outside press and
 * returns focus to the trigger.
 */
export function Popover({ trigger, children, placement = 'bottom-start', label, className, open: controlled, onOpenChange }: PopoverProps) {
  const [inner, setInner] = useState(false);
  const open = controlled ?? inner;
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const [panel, setPanel] = useState<HTMLDivElement | null>(null);
  const id = useId();
  const position = useFloating(open ? anchor : null, panel, placement);

  const setOpen = useCallback(
    (next: boolean) => {
      setInner(next);
      onOpenChange?.(next);
    },
    [onOpenChange],
  );
  const close = useCallback(() => {
    setOpen(false);
    anchor?.focus();
  }, [setOpen, anchor]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      const t = e.target as Node;
      if (!panel?.contains(t) && !anchor?.contains(t)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        close();
      }
    };
    document.addEventListener('pointerdown', onDown, true);
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('pointerdown', onDown, true);
      document.removeEventListener('keydown', onKey, true);
    };
  }, [open, panel, anchor, setOpen, close]);

  useEffect(() => {
    if (!panel || !position.ready) return;
    if (panel.contains(document.activeElement)) return;
    const first = panel.querySelector<HTMLElement>('input, select, textarea, button, [tabindex]');
    (first ?? panel).focus({ preventScroll: true });
  }, [panel, position.ready]);

  const triggerProps = trigger.props as Record<string, ((e: unknown) => void) | undefined>;
  const triggerRef = (trigger.props as { ref?: unknown }).ref;
  const setRef = useCallback(
    (el: HTMLElement | null) => {
      setAnchor(el);
      assignRef(triggerRef, el);
    },
    [triggerRef],
  );

  return (
    <>
      {cloneElement(trigger, {
        ref: setRef,
        'aria-haspopup': 'dialog',
        'aria-expanded': open,
        'aria-controls': open ? id : undefined,
        onClick: (e: unknown) => {
          triggerProps.onClick?.(e);
          setOpen(!open);
        },
      })}
      {open &&
        createPortal(
          <div
            ref={setPanel}
            id={id}
            role="dialog"
            aria-label={label}
            tabIndex={-1}
            style={{ top: position.top, left: position.left, visibility: position.ready ? 'visible' : 'hidden' }}
            className={cx('fixed z-[60] rounded-md border border-line bg-surface-2 shadow-overlay outline-none animate-pop-in', className)}
          >
            <Content render={children} close={close} />
          </div>,
          // Inside a modal <dialog> the page behind is inert and below the top layer.
          anchor?.closest('dialog') ?? document.body,
        )}
    </>
  );
}
