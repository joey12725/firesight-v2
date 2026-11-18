import { SegmentedControl } from '../SegmentedControl';
import type { TimeAxis } from './scales';
import { useChartSync, useTimeRange } from './sync';

export interface TimeRangePreset {
  id: string;
  label: string;
  /** Ages (first person). Null is the whole plan. */
  range: [number, number] | null;
}

/**
 * The view's time-range filter: presets as one segmented control, plus a note when the
 * range came from dragging on a chart. One of these sits in a row above the charts it
 * scopes; every chart that uses `useTimeRange` follows it.
 */
export function TimeRangeControl({ time, presets, className }: { time: TimeAxis; presets: TimeRangePreset[]; className?: string }) {
  const sync = useChartSync();
  const range = useTimeRange(time, sync);
  const match = presets.find((p) => (p.range === null ? !range.zoomed : range.zoomed && Math.abs(p.range[0] - range.domain[0]) < 0.6 && Math.abs(p.range[1] - range.domain[1]) < 0.6));
  const options = presets.map((p) => ({ value: p.id, label: p.label }));
  if (!match) options.push({ value: '__custom', label: `Ages ${Math.floor(range.domain[0])}–${Math.floor(range.domain[1])}` });
  return (
    <div className={className}>
      <SegmentedControl
        aria-label="Years shown"
        size="sm"
        value={match?.id ?? '__custom'}
        options={options}
        onChange={(id) => {
          const preset = presets.find((p) => p.id === id);
          if (preset) range.set(preset.range);
        }}
      />
    </div>
  );
}
