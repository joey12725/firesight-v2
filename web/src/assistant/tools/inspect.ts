// One simulated life, close up: the year-by-year ledger, the events along the way, and
// the tax return of any year.
import type { PathScalar, PathTrace, TraceColumn } from '@/engine/output-types';
import { pathAtQuantile } from './digest';
import type { RunRecord } from './runs';
import { explainEngineError } from './shared';
import { ToolError, type ToolContext, type ToolSpec } from './types';

const PICKS = ['p10', 'median', 'p90', 'median_fi', 'failing'] as const;
const BY = ['terminal_net_worth', 'fi_age', 'retirement_age', 'depletion_age', 'death_age_0', 'investable_at_retirement', 'early_retirement_return'] as const;

const RUN_ID = {
  type: 'string',
  description: 'A run id from run_simulation or try_scenario (base_run_id / scenario_run_id). Omit for the most recent run of the current plan.',
} as const;

function findRun(ctx: ToolContext, id: string | undefined): RunRecord {
  const run = ctx.runs.get(id ?? ctx.runs.latestBaseline ?? '');
  if (run) return run;
  throw new ToolError(
    id ? `There is no run "${id}" in memory. Runs available: ${ctx.runs.ids().join(', ') || 'none'}. Call run_simulation or try_scenario first.` : 'Nothing has been simulated in this conversation yet. Call run_simulation first.',
  );
}

async function traceOf(ctx: ToolContext, run: RunRecord, path: number): Promise<{ trace: PathTrace; id: string }> {
  const key = `trace:${run.id}:${path}`;
  const cached = ctx.runs.getData(key) as PathTrace | undefined;
  if (cached) return { trace: cached, id: key };
  try {
    const trace = await ctx.engine.pool.trace(run.profile, path, run.seed);
    ctx.runs.setData(key, trace);
    return { trace, id: key };
  } catch (error) {
    throw explainEngineError(error);
  }
}

const thousands = (v: number | null | undefined) => (typeof v === 'number' && Number.isFinite(v) ? Math.round(v / 1000) : null);

function col(columns: TraceColumn[], key: string): (number | null)[] | undefined {
  return columns.find((c) => c.key === key)?.values;
}

function netWorth(columns: TraceColumn[], rows: number): (number | null)[] {
  const direct = col(columns, 'net_worth');
  if (direct) return direct;
  const plus = ['balance_cash', 'balance_taxable', 'balance_traditional', 'balance_roth', 'balance_hsa', 'property_value'];
  const minus = ['mortgage', 'debt'];
  return Array.from({ length: rows }, (_, i) => plus.reduce((n, k) => n + (col(columns, k)?.[i] ?? 0), 0) - minus.reduce((n, k) => n + (col(columns, k)?.[i] ?? 0), 0));
}

interface InspectInput {
  run_id?: string;
  pick?: (typeof PICKS)[number];
  quantile?: number;
  by?: (typeof BY)[number];
  path?: number;
}

const SCALARS_SHOWN: PathScalar[] = ['fi_age', 'retirement_age', 'depletion_age', 'death_age_0', 'death_age_1', 'investable_at_retirement', 'early_retirement_return', 'worst_drawdown', 'months_unemployed', 'long_term_care_cost', 'inflation', 'terminal_net_worth', 'lifetime_tax', 'delivered_spending_ratio', 'worst_spending_ratio', 'years_depleted', 'success'];

export const inspectLife: ToolSpec<InspectInput> = {
  name: 'inspect_life',
  description:
    'Follow one simulated life year by year, to explain a result with a concrete story or to find out why a life went wrong. Choose the life by a named pick (p10, median or p90 by final net worth; median_fi; failing: a typical life that ran short), by a quantile of a per-life quantity, or by a path id from an earlier result. Returns a table of age, gross income, spending, tax, net worth (thousands of today\'s dollars) and status for every year, the dated event log (layoffs, market moves, retirement, home purchases, spending cuts, depletion, deaths), and the life\'s summary figures. It is one life out of thousands: say so when you tell its story.',
  input_schema: {
    type: 'object',
    properties: {
      run_id: RUN_ID,
      pick: { type: 'string', enum: PICKS, description: 'A representative life chosen by the engine.' },
      quantile: { type: 'number', minimum: 0, maximum: 1, description: 'With by: the life at this quantile (0.05 = a bad outcome, 0.5 = typical).' },
      by: { type: 'string', enum: BY, description: 'The per-life quantity the quantile refers to.' },
      path: { type: 'integer', minimum: 0, description: 'A specific path id.' },
    },
    additionalProperties: false,
  },
  pending: () => 'Tracing one simulated life',
  async run(input, ctx) {
    const run = findRun(ctx, input.run_id);
    let path: number;
    let chosenBy: string;
    if (input.path !== undefined) {
      if (input.path >= run.paths) throw new ToolError(`Path ${input.path} does not exist: the run has paths 0 to ${run.paths - 1}.`);
      path = input.path;
      chosenBy = `path ${path}`;
    } else if (input.quantile !== undefined || input.by) {
      const by = input.by ?? 'terminal_net_worth';
      const found = pathAtQuantile(run.summary, by, input.quantile ?? 0.5);
      if (!found) throw new ToolError(`No simulated life in this run has a value for ${by}, so there is nothing to pick. Try a named pick.`);
      path = found.path;
      chosenBy = `quantile ${input.quantile ?? 0.5} of ${by}`;
    } else {
      const label = input.pick ?? 'median';
      const pick = run.summary.picks?.find((p) => p.label === label);
      if (!pick) {
        throw new ToolError(label === 'failing' ? 'No life in this run failed, so there is no failing life to show.' : `This run has no "${label}" pick. Available: ${(run.summary.picks ?? []).map((p) => p.label).join(', ') || 'none'}.`);
      }
      path = pick.path_index;
      chosenBy = `the engine's "${label}" pick`;
    }
    const { trace, id } = await traceOf(ctx, run, path);
    const annual = trace.annual;
    const rows = annual.years.length;
    const birthYear = Number(run.profile.household.people[0]?.birth.slice(0, 4));
    const events = trace.events.map((e) => ({ month: e.month, kind: e.kind, label: e.label, ...(e.amount !== null ? { amount: Math.round(e.amount) } : {}) }));
    const MAX_EVENTS = 60;
    return {
      result: {
        trace_id: id,
        run_id: run.id,
        path,
        chosen_by: chosenBy,
        one_life_of: run.paths,
        seed: run.seed,
        units: "table amounts are thousands of today's dollars; event amounts are today's dollars",
        table: {
          year: annual.years,
          age: Number.isFinite(birthYear) ? annual.years.map((y) => y - birthYear) : null,
          income: (col(annual.columns, 'gross_income') ?? []).map(thousands),
          spending: (col(annual.columns, 'spending') ?? []).map(thousands),
          tax: (col(annual.columns, 'total_tax') ?? []).map(thousands),
          net_worth: netWorth(annual.columns, rows).map(thousands),
          status: annual.status.map((s) => trace.statuses[s] ?? null),
        },
        events: events.slice(0, MAX_EVENTS),
        ...(events.length > MAX_EVENTS ? { events_omitted: events.length - MAX_EVENTS } : {}),
        scalars: Object.fromEntries(trace.scalars.filter((s) => SCALARS_SHOWN.includes(s.scalar) && s.value !== null).map((s) => [s.scalar, Number((s.value as number).toPrecision(6))])),
        tax_years_available: trace.tax_returns.length > 0 ? [trace.tax_returns[0]?.year, trace.tax_returns[trace.tax_returns.length - 1]?.year] : null,
      },
      view: { title: `Traced one simulated life (path ${path}, ${chosenBy})`, meta: { paths: 1, seed: run.seed, runs: 1 } },
    };
  },
};

export const explainTax: ToolSpec<{ run_id?: string; path: number; year: number }> = {
  name: 'explain_tax',
  description:
    "The tax return the engine computed for one simulated life in one year, as Form 1040 style lines with the engine's note on each. Use it to explain why tax is what it is in a given year (a Roth conversion, capital gains from a home sale, the ACA credit, Social Security becoming taxable). Amounts are nominal dollars of that tax year, unlike everything else; the result includes the price level to convert. Get the path id from inspect_life or a digest's trace_paths.",
  input_schema: {
    type: 'object',
    properties: { run_id: RUN_ID, path: { type: 'integer', minimum: 0, description: 'Path id of the life.' }, year: { type: 'integer', minimum: 1900, maximum: 2200, description: 'Calendar year of the return.' } },
    required: ['path', 'year'],
    additionalProperties: false,
  },
  pending: (input) => `Reading the ${input.year} tax return of one life`,
  async run(input, ctx) {
    const run = findRun(ctx, input.run_id);
    if (input.path >= run.paths) throw new ToolError(`Path ${input.path} does not exist: the run has paths 0 to ${run.paths - 1}.`);
    const { trace } = await traceOf(ctx, run, input.path);
    const found = trace.tax_returns.find((r) => r.year === input.year);
    if (!found) {
      const years = trace.tax_returns.map((r) => r.year);
      throw new ToolError(years.length ? `This life has no tax return for ${input.year}. Returns exist for ${years[0]} to ${years[years.length - 1]}.` : 'The engine recorded no tax returns for this life.');
    }
    const MAX_LINES = 70;
    return {
      result: {
        run_id: run.id,
        path: input.path,
        year: found.year,
        dollars: 'nominal dollars of the tax year',
        price_level: Number(found.price_level.toPrecision(5)),
        to_todays_dollars: 'divide by price_level',
        lines: found.lines.slice(0, MAX_LINES).map((l) => ({ key: l.key, label: l.label, amount: Math.round(l.amount), ...(l.note ? { note: l.note } : {}) })),
        ...(found.lines.length > MAX_LINES ? { lines_omitted: found.lines.length - MAX_LINES } : {}),
      },
      view: { title: `Read the ${found.year} tax return of one simulated life (path ${input.path})`, meta: { paths: 1, seed: run.seed, runs: 1 } },
    };
  },
};
