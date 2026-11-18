// Lane layout for labels along a time axis (event markers on a life's timeline). Each
// label wants to sit centred over its own x; labels that would overlap go up a row, and
// when the rows are full the least important labels are left out (their marks stay, and
// the tooltip still names them). Pure, so it can be tested without a DOM.

export interface LaneItem {
  /** Pixel x of the mark the label belongs to. */
  x: number;
  /** Label width in pixels. */
  width: number;
  /** Higher wins a place first. */
  priority: number;
}

export interface LanePlacement {
  /** Row index, 0 nearest the plot; -1 when the label was left out. */
  row: number;
  /** Left edge of the label. */
  left: number;
}

export interface LaneOptions {
  /** Horizontal extent labels must stay inside. */
  min: number;
  max: number;
  rows: number;
  /** Clear space between two labels in a row. Default 10. */
  gap?: number;
}

export function layoutLanes(items: readonly LaneItem[], { min, max, rows, gap = 10 }: LaneOptions): LanePlacement[] {
  const out: LanePlacement[] = items.map(() => ({ row: -1, left: 0 }));
  const taken: { left: number; right: number }[][] = Array.from({ length: rows }, () => []);
  const order = items.map((item, i) => ({ item, i })).sort((a, b) => b.item.priority - a.item.priority || a.item.x - b.item.x);
  for (const { item, i } of order) {
    if (item.width > max - min) continue;
    const left = Math.min(max - item.width, Math.max(min, item.x - item.width / 2));
    const right = left + item.width;
    for (let row = 0; row < rows; row++) {
      const lane = taken[row] as { left: number; right: number }[];
      if (lane.every((t) => right + gap <= t.left || left >= t.right + gap)) {
        lane.push({ left, right });
        out[i] = { row, left };
        break;
      }
    }
  }
  return out;
}
