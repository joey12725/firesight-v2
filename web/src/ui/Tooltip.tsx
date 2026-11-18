import { cloneElement, useCallback, useEffect, useId, useState, type ReactElement, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { assignRef, useFloating, type Placement } from './useFloating';

interface TooltipProps {
  content: ReactNode;
  placement?: Placement;
  /** The trigger: one element that accepts ref, hover and focus handlers. */
  children: ReactElement<Record<string, unknown>>;
}

const SHOW_DELAY = 350;

/** A short label on hover or keyboard focus. Not for essential information. */
export function Tooltip({ content, placement = 'bottom', children }: TooltipProps) {
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState<number | null>(null);
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const [tip, setTip] = useState<HTMLDivElement | null>(null);
  const id = useId();
  const position = useFloating(open ? anchor : null, tip, placement);

  // `pending` is the delay before showing; the effect owns the timer.
  useEffect(() => {
    if (pending === null) return;
    const timer = setTimeout(() => setOpen(true), pending);
    return () => clearTimeout(timer);
  }, [pending]);

  const hide = useCallback(() => {
    setPending(null);
    setOpen(false);
  }, []);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && hide();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, hide]);

  const props = children.props as Record<string, ((e: unknown) => void) | undefined>;
  const childRef = (children.props as { ref?: unknown }).ref;
  const setRef = useCallback(
    (el: HTMLElement | null) => {
      setAnchor(el);
      assignRef(childRef, el);
    },
    [childRef],
  );

  return (
    <>
      {cloneElement(children, {
        ref: setRef,
        'aria-describedby': open ? id : (children.props['aria-describedby'] as string | undefined),
        onPointerEnter: (e: unknown) => {
          props.onPointerEnter?.(e);
          if ((e as PointerEvent).pointerType !== 'touch') setPending(SHOW_DELAY);
        },
        onPointerLeave: (e: unknown) => {
          props.onPointerLeave?.(e);
          hide();
        },
        onPointerDown: (e: unknown) => {
          props.onPointerDown?.(e);
          hide();
        },
        onFocus: (e: unknown) => {
          props.onFocus?.(e);
          if ((e as { target: Element }).target.matches(':focus-visible')) setPending(0);
        },
        onBlur: (e: unknown) => {
          props.onBlur?.(e);
          hide();
        },
      })}
      {open &&
        createPortal(
          <div
            ref={setTip}
            id={id}
            role="tooltip"
            style={{ top: position.top, left: position.left, visibility: position.ready ? 'visible' : 'hidden' }}
            className="pointer-events-none fixed z-[70] max-w-64 rounded-sm bg-surface-inverse px-2 py-1 text-xs text-ink-inverse animate-fade-in"
          >
            {content}
          </div>,
          // Inside a modal <dialog> the page behind is inert and below the top layer.
          anchor?.closest('dialog') ?? document.body,
        )}
    </>
  );
}
