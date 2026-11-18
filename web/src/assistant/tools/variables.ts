// The quantities `solve` and `sweep` can vary: a few named ones that move several
// fields together, or any single numeric field by its patch path.
import { budgetTotals } from '@/editor/labels';
import type { PatchOp, Profile } from '@/engine/types';
import { getAt } from '@/state/pointer';
import type { JsonSchema } from '../agent/validate-input';
import { ToolError } from './types';

export const VARIABLES = ['annual_spending', 'retirement_age', 'withdrawal_rate', 'salary_scale'] as const;
export type VariableName = (typeof VARIABLES)[number];

export interface VariableInput {
  variable?: VariableName;
  path?: string;
}

export interface Variable {
  label: string;
  unit: 'dollars' | 'age' | 'rate' | 'factor' | 'number';
  /** The value in the plan today, when it has one. */
  current: number | null;
  bounds: [number, number];
  tolerance: number;
  ops(value: number): PatchOp[];
}

export const VARIABLE_SCHEMA_PROPERTIES: Record<string, JsonSchema> = {
  variable: {
    type: 'string',
    enum: VARIABLES,
    description:
      "annual_spending: the whole budget in today's dollars a year (every line scaled together). retirement_age: the first person's retirement age (sets the trigger to retire at that age). withdrawal_rate: the rate in a when_funded retirement trigger. salary_scale: a multiplier on every salary (1 = today).",
  },
  path: { type: 'string', description: 'Instead of variable: the patch path of one numeric field to vary, e.g. /portfolio/expense_ratio or /incomes/@alex_job/kind/salary.' },
};

const round = (v: number, step: number) => Math.round(v / step) * step;

export function resolveVariable(profile: Profile, input: VariableInput): Variable {
  if (input.variable && input.path) throw new ToolError('Give either variable or path, not both.');
  if (input.path) {
    const current = getAt(profile, input.path);
    if (typeof current !== 'number') {
      throw new ToolError(`The field at ${input.path} is ${current === undefined ? 'missing' : `not a number (${JSON.stringify(current)?.slice(0, 80)})`}. Give the path of a numeric field; get_plan shows the paths that exist.`);
    }
    const path = input.path;
    const span = Math.abs(current) > 0 ? Math.abs(current) : 1;
    return {
      label: path,
      unit: 'number',
      current,
      bounds: [current - span / 2, current + span / 2],
      tolerance: span / 200,
      ops: (value) => [{ op: 'set', path, value }],
    };
  }
  switch (input.variable) {
    case 'annual_spending': {
      const now = budgetTotals(profile).now;
      if (!(now > 0)) throw new ToolError('The plan has no budget lines, so spending cannot be varied. Add spending first.');
      return {
        label: "Annual spending (today's dollars)",
        unit: 'dollars',
        current: Math.round(now),
        bounds: [round(now * 0.5, 100), round(now * 1.5, 100)],
        tolerance: Math.max(100, round(now * 0.005, 100)),
        ops: (value) => profile.spending.items.map((item, i) => ({ op: 'set', path: `/spending/items/${i}/amount`, value: (item.amount * value) / now })),
      };
    }
    case 'retirement_age': {
      const trigger = profile.retirement.trigger;
      const birth = profile.household.people[0]?.birth;
      const startYear = Number((profile.start ?? '').slice(0, 4)) || new Date().getFullYear();
      const ageNow = birth ? startYear - Number(birth.slice(0, 4)) : 30;
      return {
        label: "Retirement age (first person)",
        unit: 'age',
        current: trigger.type === 'at_age' ? trigger.age : null,
        bounds: [Math.min(69, Math.max(40, ageNow + 1)), 70],
        tolerance: 0.25,
        ops: (value) => [{ op: 'set', path: '/retirement/trigger', value: { type: 'at_age', person: 0, age: Math.round(value * 4) / 4 } }],
      };
    }
    case 'withdrawal_rate': {
      const trigger = profile.retirement.trigger;
      if (trigger.type !== 'when_funded') {
        throw new ToolError(`withdrawal_rate applies only when the retirement trigger is when_funded; this plan's trigger is ${trigger.type}. Vary retirement_age instead, or a path.`);
      }
      return {
        label: 'Withdrawal rate that triggers retirement',
        unit: 'rate',
        current: trigger.withdrawal_rate,
        bounds: [0.02, 0.07],
        tolerance: 0.0005,
        ops: (value) => [{ op: 'set', path: '/retirement/trigger/withdrawal_rate', value }],
      };
    }
    case 'salary_scale': {
      const jobs = profile.incomes.flatMap((income, i) => (income.kind.type === 'job' ? [{ i, salary: income.kind.salary }] : []));
      if (jobs.length === 0) throw new ToolError('The plan has no job income, so salaries cannot be varied.');
      return {
        label: 'Salaries (multiple of today)',
        unit: 'factor',
        current: 1,
        bounds: [0.5, 1.5],
        tolerance: 0.01,
        ops: (value) => jobs.map(({ i, salary }) => ({ op: 'set', path: `/incomes/${i}/kind/salary`, value: salary * value })),
      };
    }
    default:
      throw new ToolError(`Give a variable (${VARIABLES.join(', ')}) or the path of a numeric field.`);
  }
}

export function roundValue(variable: Variable, value: number): number {
  switch (variable.unit) {
    case 'dollars':
      return Math.round(value / 100) * 100;
    case 'age':
      return Math.round(value * 4) / 4;
    case 'rate':
      return Math.round(value * 10_000) / 10_000;
    case 'factor':
      return Math.round(value * 1_000) / 1_000;
    default:
      return Number(value.toPrecision(6));
  }
}
