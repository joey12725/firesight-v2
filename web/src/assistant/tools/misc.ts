import { relocationFactors, searchCities, type City } from '@/editor/cities';
import { CHART_KINDS, CHART_SPECS, type ChartKind } from '../charts';
import { ToolError, type ToolSpec } from './types';

function match(cities: City[], query: string): { city: City | null; candidates: City[] } {
  const [name = '', ...rest] = query.split(',').map((s) => s.trim());
  const region = rest.join(', ').toLowerCase();
  const exact = cities.filter((c) => c.city.toLowerCase() === name.toLowerCase());
  const inRegion = region ? exact.filter((c) => c.country.toLowerCase() === region || c.countryCode.toLowerCase() === region || c.search.includes(region)) : exact;
  if (inRegion.length === 1) return { city: inRegion[0] as City, candidates: [] };
  if (exact.length === 1) return { city: exact[0] as City, candidates: [] };
  const candidates = exact.length > 1 ? exact : searchCities(cities, name, 6);
  return { city: null, candidates };
}

const display = (c: City) => `${c.city}, ${c.country}`;

export const lookupCostOfLiving: ToolSpec<{ from: string; to: string; family?: boolean }> = {
  name: 'lookup_cost_of_living',
  description:
    'Look up how prices differ between two cities in the app\'s cost-of-living table, as per-category multipliers (destination over origin) ready to use in a relocate event. Use this instead of your own impression of what places cost. Categories priced nationally or by contract (healthcare, insurance, giving, education, travel) are left out and stay unchanged. If a city is not found, the closest names in the table are returned.',
  input_schema: {
    type: 'object',
    properties: {
      from: { type: 'string', minLength: 2, description: 'Origin city, optionally with country: "Denver" or "Portland, United States".' },
      to: { type: 'string', minLength: 2, description: 'Destination city.' },
      family: { type: 'boolean', description: 'Use the family rather than the single-person basket for the catch-all category. Default: true when the plan has a partner or children.' },
    },
    required: ['from', 'to'],
    additionalProperties: false,
  },
  pending: (input) => `Looking up prices in ${input.to}`,
  async run(input, ctx) {
    let cities: City[];
    try {
      cities = await ctx.cities();
    } catch {
      throw new ToolError('The cost-of-living table could not be loaded. Say so, and ask the user for their own estimate of the price difference rather than guessing.');
    }
    const from = match(cities, input.from);
    const to = match(cities, input.to);
    if (!from.city || !to.city) {
      return {
        result: {
          found: false,
          ...(from.city ? {} : { from_not_found: input.from, from_candidates: from.candidates.map(display) }),
          ...(to.city ? {} : { to_not_found: input.to, to_candidates: to.candidates.map(display) }),
          note: 'Call again with one of the candidates, or ask the user which city they mean. If there is no close match, ask for their own estimate.',
        },
        isError: true,
        view: { title: 'City not found in the cost-of-living table' },
      };
    }
    const profile = ctx.store.profile();
    const family = input.family ?? (profile ? profile.household.people.length > 1 || profile.household.children.length > 0 : false);
    const factors = relocationFactors(from.city, to.city, family);
    return {
      result: {
        found: true,
        from: display(from.city),
        to: display(to.city),
        country_code: to.city.countryCode,
        basket: family ? 'family' : 'single',
        factors,
        relocate_cost_of_living: { type: 'by_category', factors },
        home_price_ratio: Math.round((to.city.homePrice / from.city.homePrice) * 1000) / 1000,
        data_quality: from.city.quality >= 1 && to.city.quality >= 1 ? 'good' : 'thin: few contributors for at least one city',
        note: 'A factor of 0.8 means that category costs 20% less at the destination. Tax differences are computed by the engine from the new location, not by these factors.',
      },
      view: { title: `Looked up prices: ${display(from.city)} to ${display(to.city)}` },
    };
  },
};

interface ShowChartInput {
  kind: ChartKind;
  caption: string;
  run_id?: string;
  compare_run_id?: string;
  data_id?: string;
}

export const showChart: ToolSpec<ShowChartInput> = {
  name: 'show_chart',
  description: `Show a chart in the conversation, drawn by the app from a result you already have. You choose the kind and what it refers to; you cannot supply data. Kinds: ${CHART_KINDS.map((k) => `${k} (${CHART_SPECS[k].use}; needs ${CHART_SPECS[k].needs})`).join('; ')}. Use a chart when the shape matters more than a number (a distribution, a trade-off, a life), at most one or two per answer, and say in the caption what to look at.`,
  input_schema: {
    type: 'object',
    properties: {
      kind: { type: 'string', enum: CHART_KINDS },
      caption: { type: 'string', minLength: 1, maxLength: 200, description: 'One sentence under the chart saying what it shows and what to notice. No figures that are not in a tool result.' },
      run_id: { type: 'string', description: 'The run to draw. Omit for the most recent run of the current plan.' },
      compare_run_id: { type: 'string', description: 'For compare_delta, and optionally fan or fi_age: the scenario run to draw against run_id.' },
      data_id: { type: 'string', description: 'ranking_id (tornado), grid_id (heatmap) or trace_id (life_story, cash_flow of one life).' },
    },
    required: ['kind', 'caption'],
    additionalProperties: false,
  },
  pending: () => 'Preparing a chart',
  async run(input, ctx) {
    const spec = CHART_SPECS[input.kind];
    const runId = input.run_id ?? ctx.runs.latestBaseline;
    const known = () => ctx.runs.ids().join(', ') || 'none';
    if (input.run_id && !ctx.runs.get(input.run_id)) throw new ToolError(`There is no run "${input.run_id}" in memory. Runs: ${known()}.`);
    if (input.compare_run_id && !ctx.runs.get(input.compare_run_id)) throw new ToolError(`There is no run "${input.compare_run_id}" in memory. Runs: ${known()}.`);
    if (spec.source === 'run' && (!runId || !ctx.runs.get(runId))) throw new ToolError('Nothing has been simulated yet. Call run_simulation first, then show the chart.');
    if (spec.source === 'comparison' && !(runId && ctx.runs.get(runId) && input.compare_run_id)) {
      throw new ToolError('compare_delta needs run_id and compare_run_id from a try_scenario result (base_run_id and scenario_run_id).');
    }
    if (spec.source === 'data') {
      const data = input.data_id ? ctx.runs.getData(input.data_id) : undefined;
      const prefix = spec.dataPrefix ?? '';
      if (!input.data_id || data === undefined || !input.data_id.startsWith(prefix)) {
        throw new ToolError(`${input.kind} needs data_id: ${spec.needs}.`);
      }
    }
    return {
      result: { shown: true, kind: input.kind },
      view: {
        title: `Showed a chart: ${spec.label.toLowerCase()}`,
        display: { type: 'chart', kind: input.kind, caption: input.caption, runId: spec.source === 'data' ? null : (runId ?? null), compareRunId: input.compare_run_id ?? null, dataId: input.data_id ?? null },
      },
    };
  },
};
