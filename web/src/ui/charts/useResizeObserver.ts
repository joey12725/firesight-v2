import { useLayoutEffect, useState, type RefObject } from 'react';

export interface Size {
  width: number;
  height: number;
}

/** The content-box size of an element in CSS pixels, updated as it resizes. */
export function useResizeObserver(ref: RefObject<Element | null>): Size {
  const [size, setSize] = useState<Size>({ width: 0, height: 0 });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const { width, height } = entry.contentRect;
      setSize((prev) =>
        Math.round(prev.width) === Math.round(width) && Math.round(prev.height) === Math.round(height)
          ? prev
          : { width, height },
      );
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref]);
  return size;
}
