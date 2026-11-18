import { useLayoutEffect, useState } from 'react';

export type Placement = 'top' | 'bottom' | 'bottom-start' | 'bottom-end' | 'right' | 'left';

interface Position {
  top: number;
  left: number;
  /** False until measured, so the first frame can be hidden. */
  ready: boolean;
}

const GAP = 6;
const MARGIN = 8;
const HIDDEN: Position = { top: 0, left: 0, ready: false };

/**
 * Positions a floating element against an anchor with fixed coordinates, flipping and
 * shifting to stay inside the viewport. Recomputed on scroll and resize while open.
 * Pass elements (from callback refs held in state), not ref objects.
 */
export function useFloating(anchor: HTMLElement | null, floating: HTMLElement | null, placement: Placement): Position {
  const [position, setPosition] = useState<Position>(HIDDEN);

  useLayoutEffect(() => {
    if (!anchor || !floating) return;
    const update = () => {
      const a = anchor.getBoundingClientRect();
      const f = floating.getBoundingClientRect();
      const vw = window.innerWidth;
      const vh = window.innerHeight;
      let top: number;
      let left: number;
      if (placement === 'right' || placement === 'left') {
        top = a.top + a.height / 2 - f.height / 2;
        left = placement === 'right' ? a.right + GAP : a.left - GAP - f.width;
        if (left + f.width > vw - MARGIN) left = a.left - GAP - f.width;
        if (left < MARGIN) left = a.right + GAP;
      } else {
        const below = placement !== 'top';
        top = below ? a.bottom + GAP : a.top - GAP - f.height;
        if (below && top + f.height > vh - MARGIN && a.top - GAP - f.height > MARGIN) top = a.top - GAP - f.height;
        if (!below && top < MARGIN) top = a.bottom + GAP;
        left = placement === 'bottom-start' ? a.left : placement === 'bottom-end' ? a.right - f.width : a.left + a.width / 2 - f.width / 2;
      }
      left = Math.max(MARGIN, Math.min(left, vw - f.width - MARGIN));
      top = Math.max(MARGIN, Math.min(top, vh - f.height - MARGIN));
      setPosition((p) => (p.ready && p.top === top && p.left === left ? p : { top, left, ready: true }));
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(floating);
    window.addEventListener('scroll', update, true);
    window.addEventListener('resize', update);
    return () => {
      observer.disconnect();
      window.removeEventListener('scroll', update, true);
      window.removeEventListener('resize', update);
      setPosition(HIDDEN);
    };
  }, [anchor, floating, placement]);

  return position;
}

/** Sets a ref of either kind. For forwarding an element to a ref the child already had. */
export function assignRef<T>(ref: unknown, value: T | null) {
  if (typeof ref === 'function') (ref as (v: T | null) => void)(value);
  else if (ref && typeof ref === 'object') (ref as { current: T | null }).current = value;
}
