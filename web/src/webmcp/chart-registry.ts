// The charts on screen, as data. Every `ChartCard` (web/src/ui/charts) announces itself
// here while it is mounted, so that the WebMCP tools `list_charts` and `get_chart_data`
// can hand an agent what a person reads off the chart: its title, its one-sentence
// takeaway, its caption and the rows behind it. A WebMCP result is text only, so this is
// how an agent "sees" a chart. Nothing reads the registry unless such a tool is called.
export interface ChartTableColumn {
  key: string;
  label: string;
}

export interface ChartTable {
  caption: string;
  columns: ChartTableColumn[];
  rows: Record<string, number | string | null>[];
}

export interface ChartEntry {
  /** File-name stem of the card, unique among the charts on screen. */
  id: string;
  title: string;
  /** Read when asked, so they are always what is on screen now. */
  takeaway(): string | null;
  caption(): string | null;
  /** Null when the chart has no table view. */
  table: (() => ChartTable) | null;
}

interface Registered extends Omit<ChartEntry, 'id'> {
  /** File-name stem of the card. */
  stem: string;
  /**
   * Where the chart is. `shown`: in the results view and drawn. `hidden`: in the results view but not drawn
   * (React keeps the previous view mounted, hidden, while the next one loads). `elsewhere`: not a chart of
   * the view at all (one in an assistant message). Omitted, the chart counts as shown while registered.
   */
  place?: () => 'shown' | 'hidden' | 'elsewhere';
}

const mounted = new Map<symbol, Registered>();

/** Adds a chart for as long as it is mounted. Returns the function that removes it. */
export function registerChart(entry: Registered): () => void {
  const key = Symbol(entry.stem);
  mounted.set(key, entry);
  return () => {
    mounted.delete(key);
  };
}

/** The charts on screen in mount order, with repeated stems numbered (`net-worth`, `net-worth-2`). */
export function mountedCharts(): ChartEntry[] {
  const seen = new Map<string, number>();
  const shown = [...mounted.values()].filter((entry) => (entry.place?.() ?? 'shown') === 'shown').map(({ stem, place: _place, ...rest }): ChartEntry => ({ ...rest, id: stem }));
  return shown.map((entry) => {
    const n = (seen.get(entry.id) ?? 0) + 1;
    seen.set(entry.id, n);
    return n === 1 ? entry : { ...entry, id: `${entry.id}-${n}` };
  });
}

/** True while the results view holds charts it is not showing: the view that was asked for is still loading. */
export function chartsPending(): boolean {
  return [...mounted.values()].some((entry) => entry.place?.() === 'hidden');
}
