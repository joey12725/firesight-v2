// The app's own run, in the few figures a person reads off the page. An agent gets these
// instead of the run's Summary, which is hundreds of kilobytes.
import { digestComparison } from '@/assistant/tools/digest';
import type { Comparison, Estimate, NullableEstimate, PlanPreview, Spread, Summary } from '@/engine/output-types';

const round = (value: number | null | undefined, digits: number): number | null => {
  if (value === null || value === undefined || !Number.isFinite(value)) return null;
  const f = 10 ** digits;
  return Math.round(value * f) / f;
};
const dollars = (value: number | null | undefined): number | null => (value === null || value === undefined || !Number.isFinite(value) ? null : Math.round(value / 1000) * 1000);

const probability = (e: Estimate) => ({ p: round(e.value, 3), lo: round(e.lo, 3), hi: round(e.hi, 3) });
const share = (e: NullableEstimate) => round(e.value, 3);
const ages = (s: Spread) => ({ p10: round(s.p10, 1), p50: round(s.p50, 1), p90: round(s.p90, 1) });
const money = (s: Spread) => ({ p10: dollars(s.p10), p50: dollars(s.p50), p90: dollars(s.p90) });

/** The headline strip of the results workspace, as numbers. */
export function headlineFigures(summary: Summary) {
  const h = summary.headline;
  return {
    lives: summary.paths,
    success: probability(h.success_rate),
    home_sale: round(h.home_sale_rate.value, 3),
    fi: probability(h.fi_rate),
    depletion: probability(h.depletion_rate),
    fi_age: ages(h.fi_age),
    retirement_age: ages(h.retirement_age),
    depletion_age_p50: round(h.depletion_age.p50, 1),
    terminal_net_worth: money(h.terminal_net_worth),
    investable_at_retirement: money(h.investable_at_retirement),
    spending: {
      plan_delivered: share(h.plan_delivered_share),
      extra_ratio: share(h.extra_spending_ratio),
      years_below_90: share(summary.spending.years_below_90),
    },
    median_lifetime_tax: dollars(h.median_lifetime_tax),
  };
}

/** What the plan implies before any simulation: the line under the headline strip. */
export function planFacts(preview: PlanPreview) {
  return {
    annual_spending: dollars(preview.annual_spending),
    annual_spending_retired: dollars(preview.annual_spending_retired),
    annual_income: dollars(preview.annual_income),
    annual_tax: dollars(preview.annual_tax),
    savings_rate: round(preview.savings_rate, 3),
    investable: dollars(preview.investable),
    funded_requirement: dollars(preview.funded_requirement),
    funded_ratio: round(preview.funded_ratio, 3),
  };
}

/** One enabled scenario beside the plan: its own headline, and the paired difference when it is ready. */
export function scenarioFigures(summary: Summary | null, paired: Comparison | null, seed: number) {
  const c = paired ? digestComparison(paired, seed) : null;
  return {
    ...(summary
      ? {
          success: probability(summary.headline.success_rate),
          home_sale: round(summary.headline.home_sale_rate.value, 3),
          depletion: probability(summary.headline.depletion_rate),
          fi_age_p50: round(summary.headline.fi_age.p50, 1),
        }
      : {}),
    ...(c ? { versus_plan: { paired: true, paired_paths: c.paired_paths, success: c.success, depleted: c.depleted, home_sale: c.home_sale } } : {}),
  };
}

export const RESULT_NOTES =
  "Money is in today's dollars, rounded to the nearest $1,000. Ages are the first person's. {p, lo, hi} is a probability with its 95% sampling interval. success counts a home sold as a last resort as the plan holding: state home_sale (the share of lives in which that happens) with it. A null p50 for fi_age or retirement_age means fewer than half of the lives get there. plan_delivered is the share of planned retirement spending delivered (never above 1); extra_ratio is spending above plan, reported apart. versus_plan is the scenario minus the plan on the same simulated lives; within_noise: true means the difference cannot be told from zero.";
