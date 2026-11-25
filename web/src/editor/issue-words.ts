// The engine words a range error for the file format, where rates and fractions are
// decimals ("it must be between 0 and 1 ... write 0.05 for 5%"). Under a field that is
// typed and shown in percent that tells the reader to make the entry worse, so the
// editor says the same thing in the field's own unit.
import type { FieldIssue } from '@/ui';
import { percent } from '@/ui/format';

const RANGE = /^(.*) is (-?\d+(?:\.\d+)?(?:e[+-]?\d+)?); it must be between (-?\d+(?:\.\d+)?) and (-?\d+(?:\.\d+)?)\.( Rates and fractions are decimals: write 0\.05 for 5%\.)?$/i;

/** The engine's out-of-range message for a percent field, in percent: "Tender limit is 150%; it must be between 0% and 100%." */
export function percentRangeMessage(message: string): string {
  const m = RANGE.exec(message);
  if (!m) return message;
  const [, what, value, lo, hi] = m;
  const pct = (text: string | undefined) => percent(Number(text), 4);
  return `${what} is ${pct(value)}; it must be between ${pct(lo)} and ${pct(hi)}.`;
}

/** Issues of a percent field, with range errors reworded in percent. The same array when nothing changes. */
export function percentIssues(issues: FieldIssue[]): FieldIssue[] {
  if (!issues.some((i) => RANGE.test(i.message))) return issues;
  return issues.map((i) => ({ ...i, message: percentRangeMessage(i.message) }));
}

/** Under "Linked job" when the job a company points at is gone. The engine's own text names the missing id, which the household never saw. */
export const MISSING_JOB = 'The job this company was linked to has been removed. Choose another job, or None if you no longer work there.';

/** An engine issue as the editor's Plan check words it: percent for a fraction typed over 1, and the removed-job case. */
export function editorMessage(issue: { code: string; path: string; message: string }): string {
  if (issue.code === 'ref_income' && /^\/private_equity\/[^/]+\/job$/.test(issue.path) && /^No income with id/.test(issue.message)) return MISSING_JOB;
  // Only a fraction typed above 1 carries the decimals hint, and the editor shows every such field in percent.
  if (issue.code === 'out_of_range' && /Rates and fractions are decimals/.test(issue.message)) return percentRangeMessage(issue.message);
  return issue.message;
}
