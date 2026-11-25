import { describe, expect, it } from 'vitest';
import { MISSING_JOB, editorMessage, percentIssues, percentRangeMessage } from './issue-words';

// The engine's own wording (core `validate.rs`, `range`).
const LIMIT = 'Tender limit is 1.5; it must be between 0 and 1. Rates and fractions are decimals: write 0.05 for 5%.';

describe('range errors under a percent field', () => {
  it('are said in percent, without the advice to write a decimal', () => {
    expect(percentRangeMessage(LIMIT)).toBe('Tender limit is 150%; it must be between 0% and 100%.');
    expect(percentRangeMessage('Tender price fraction is 0.01; it must be between 0.05 and 2.')).toBe('Tender price fraction is 1%; it must be between 5% and 200%.');
    expect(percentRangeMessage('Preference overhang is -0.2; it must be between 0 and 0.95.')).toBe('Preference overhang is -20%; it must be between 0% and 95%.');
    // Anything else is left as the engine wrote it.
    expect(percentRangeMessage('The cliff cannot be later than the end of vesting.')).toBe('The cliff cannot be later than the end of vesting.');
  });

  it('keep the severity and leave other issues alone', () => {
    const other = { severity: 'warning' as const, message: 'This is unusual.' };
    expect(percentIssues([{ severity: 'error', message: LIMIT }, other])).toEqual([{ severity: 'error', message: 'Tender limit is 150%; it must be between 0% and 100%.' }, other]);
    const untouched = [other];
    expect(percentIssues(untouched)).toBe(untouched);
  });
});

describe('the plan check', () => {
  it('words a fraction typed over 1 in percent, and a removed job without its id', () => {
    expect(editorMessage({ code: 'out_of_range', path: '/private_equity/@c/tenders/max_fraction', message: LIMIT })).toBe('Tender limit is 150%; it must be between 0% and 100%.');
    const gone = { code: 'ref_income', path: '/private_equity/@company/job', message: 'No income with id "new_job". Income ids: new_job_2.' };
    expect(editorMessage(gone)).toBe(MISSING_JOB);
    expect(MISSING_JOB).not.toMatch(/new_job|id\b/);
    // The same code elsewhere, and a range that is not a fraction, keep the engine's text.
    const elsewhere = { ...gone, path: '/events/@e/income' };
    expect(editorMessage(elsewhere)).toBe(gone.message);
    const months = { code: 'out_of_range', path: '/x', message: 'Lockup is 30; it must be between 0 and 24.' };
    expect(editorMessage(months)).toBe(months.message);
  });
});
