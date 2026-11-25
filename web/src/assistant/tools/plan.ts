import { toEngineError } from '@/engine/errors';
import type { PlanPreview } from '@/engine/output-types';
import type { PatchOp } from '@/engine/types';
import { describeRoot, describeType, diffEntries, newIssues, planView, referencedTypes, typeNames, type SchemaNode } from './plan-view';
import { asOps, PATCH_OP_SCHEMA, requireProfile } from './shared';
import { ToolError, type Assumption, type Proposal, type ToolContext, type ToolSpec } from './types';

/** The current plan as the model should see it. Also attached to user turns when the plan has changed. */
export function currentPlanView(ctx: ToolContext) {
  const profile = requireProfile(ctx);
  // The preview needs a plan the engine can compile; one with errors is shown without it.
  let preview: PlanPreview | null;
  try {
    preview = ctx.engine.planPreview(profile);
  } catch {
    preview = null;
  }
  return planView({
    profile,
    report: ctx.store.report(),
    scenarios: ctx.store.scenarios(),
    run: ctx.store.run(),
    schema: ctx.engine.schema() as SchemaNode,
    today: ctx.today(),
    preview,
    equityPreview: (company) => ctx.engine.equityPreview(company),
  });
}

export const getPlan: ToolSpec<Record<string, never>> = {
  name: 'get_plan',
  description:
    "Read the user's current plan: every non-default field with the ids that patches address, the engine's plan preview (what the plan implies before any simulation: annual_spending now and annual_spending_retired, each the whole cost of living including housing, modelled healthcare, children and debt payments; annual_income; annual_tax; savings_rate; investable; funded_requirement, the wealth at which the retirement trigger's funded test is met today, with funded_ratio; life expectancy; left out while the plan has errors), balances by account kind, for each private company under derived.private_equity its paper value, exercise cost and the odds in force (odds_in_force and typical_odds in futures of 100 over the next ten years, the implied multiples, nothing_share and the 409A fraction), the validation report (errors block simulation; warnings are worth raising), the saved scenarios and the run settings. The plan is also attached to the user's message whenever it has changed, so call this only when you need to re-read it after edits made during this turn.",
  input_schema: { type: 'object', properties: {}, additionalProperties: false },
  pending: () => 'Reading the plan',
  async run(_input, ctx) {
    const view = currentPlanView(ctx);
    return { result: view, view: { title: 'Read the plan' } };
  },
};

/** Hand-kept so the tool definition (part of the cached prompt) never changes at runtime; a test checks it against the schema. */
export const SCHEMA_TYPES = [
  'Profile', 'Household', 'Person', 'Sex', 'HealthStatus', 'SocialSecurity', 'SocialSecurityBasis', 'Child', 'ChildCosts', 'College', 'Location',
  'Income', 'IncomeKind', 'WageGrowth', 'LifecycleProfile', 'CareerRisk', 'RiskLevel', 'Bonus', 'Commission', 'EquityGrant', 'EquityKind',
  'EquityCompany', 'EmployerPlan', 'Cola', 'When', 'PrivateCompany', 'CompanyStage', 'Outlook', 'OutcomeRow', 'OutcomeKind', 'TenderOffers',
  'EquityHolding', 'HoldingKind', 'EquityPolicy', 'ExerciseTiming', 'DeadlinePolicy', 'SellDown', 'Spending', 'BudgetItem', 'Category', 'Frequency', 'RetirementChange', 'ShockLevel',
  'Healthcare', 'HealthcareMode', 'EarlyRetirementCoverage', 'PlanTier', 'Account', 'AccountKind', 'CashYield', 'Allocation', 'PortfolioPolicy',
  'GlidePath', 'GlidePoint', 'Rebalance', 'AssetLocation', 'CustomAsset', 'SingleStock', 'Property', 'PropertyUse', 'Mortgage', 'Debt',
  'DebtKind', 'DebtPolicy', 'DebtStrategy', 'LifeEvent', 'EventKind', 'CostOfLivingChange', 'Financing', 'IncomeChangeKind', 'SpendingTarget',
  'SpendingChangeKind', 'RetirementPlan', 'RetirementTrigger', 'WealthBasis', 'SpendingRule', 'WithdrawalOrder', 'EarlyAccess', 'HomeEquity', 'Assumptions',
  'MarketModel', 'MarketKind', 'Stress', 'TaxModel', 'TaxKind', 'SocialSecurityHaircut', 'Longevity', 'SimulationSettings',
] as const;

const MAX_SCHEMA_CHARS = 7_500;

export const getSchema: ToolSpec<{ section: string; with_nested?: boolean }> = {
  name: 'get_schema',
  description:
    'Look up exact field names, types, allowed values, defaults and documentation for one type of the plan schema before writing a patch you are not certain of. The schema map in your instructions names the types. "Profile" lists the top-level fields. Set with_nested to also get the types the section embeds (for example Income with IncomeKind, WageGrowth, EmployerPlan).',
  input_schema: {
    type: 'object',
    properties: {
      section: { type: 'string', enum: SCHEMA_TYPES, description: 'Schema type name.' },
      with_nested: { type: 'boolean', description: 'Include the types this one refers to, one level deep. Default false.' },
    },
    required: ['section'],
    additionalProperties: false,
  },
  pending: (input) => `Looking up ${input.section}`,
  async run(input, ctx) {
    const root = ctx.engine.schema() as SchemaNode;
    const main = input.section === 'Profile' ? describeRoot(root) : describeType(root, input.section);
    if (!main) throw new ToolError(`There is no schema type "${input.section}". Types: ${typeNames(root).join(', ')}`);
    const nested: unknown[] = [];
    let size = JSON.stringify(main).length;
    const omitted: string[] = [];
    if (input.with_nested && input.section !== 'Profile') {
      for (const name of referencedTypes(root, input.section)) {
        const described = describeType(root, name);
        const length = JSON.stringify(described).length;
        if (size + length > MAX_SCHEMA_CHARS) omitted.push(name);
        else {
          nested.push(described);
          size += length;
        }
      }
    }
    return {
      result: { ...(main as object), ...(nested.length ? { nested } : {}), ...(omitted.length ? { not_included_ask_separately: omitted } : {}) },
      view: { title: `Looked up ${input.section} in the schema` },
    };
  },
};

interface ProposeInput {
  summary: string;
  ops: PatchOp[];
  assumptions?: Assumption[];
}

export const proposeChanges: ToolSpec<ProposeInput> = {
  name: 'propose_changes',
  description:
    "Propose edits to the user's baseline plan. This is the only way the plan changes. The edits are validated first: if the patch does not apply, or would make the plan invalid, the error comes back verbatim so you can correct the patch and call again (the user is not shown failed attempts). Otherwise the user sees a card listing each edit as before and after, with your assumptions, and accepts, edits or declines; the result tells you which, and the edits they made. Group edits that belong together into one call (a whole person, a whole job) and keep unrelated edits in separate calls so each can be judged on its own. List under assumptions every value the user did not state outright.",
  input_schema: {
    type: 'object',
    properties: {
      summary: { type: 'string', minLength: 1, maxLength: 160, description: 'One plain sentence naming what changes, shown as the card title and the undo label. No figures that are not in the ops.' },
      ops: { type: 'array', minItems: 1, maxItems: 60, items: PATCH_OP_SCHEMA, description: 'Patch operations, applied in order to the baseline plan.' },
      assumptions: {
        type: 'array',
        maxItems: 30,
        description: 'One entry per value you chose rather than were told, plus the stated values that matter most.',
        items: {
          type: 'object',
          properties: {
            field: { type: 'string', description: 'What the value is, in the user\'s words: "Retirement contribution rate".' },
            value: { type: 'string', description: 'The value as you set it, readable: "6% of salary".' },
            basis: { type: 'string', enum: ['stated', 'inferred', 'default'], description: 'stated: the user or their document said so. inferred: you derived it from what they said. default: you left or chose a typical value.' },
            confidence: { type: 'string', enum: ['high', 'medium', 'low'] },
          },
          required: ['field', 'value', 'basis', 'confidence'],
          additionalProperties: false,
        },
      },
    },
    required: ['summary', 'ops'],
    additionalProperties: false,
  },
  pending: () => 'Drafting changes to the plan',
  async run(input, ctx, call) {
    const profile = requireProfile(ctx);
    const ops = asOps(input.ops);
    const before = ctx.store.report();
    const preview = ctx.store.preview(ops);
    if ('error' in preview) throw new ToolError(preview.error, 'A proposed edit did not apply');
    const newErrors = newIssues(before.errors, preview.report.errors);
    if (newErrors.length > 0) {
      throw new ToolError(`The patch applies, but the plan would then have errors that stop it from being simulated. Fix the patch and propose again. ${newErrors.slice(0, 8).join(' | ')}`, 'A proposed edit would make the plan invalid');
    }
    const proposal: Proposal = {
      summary: input.summary,
      ops,
      entries: diffEntries(profile, ops),
      assumptions: input.assumptions ?? [],
      newWarnings: newIssues(before.warnings, preview.report.warnings),
    };

    const apply = (final: PatchOp[]) => {
      const result = ctx.store.edit(final, input.summary);
      if (!result.ok) throw new ToolError(`The change could not be applied: ${result.error}`, 'The change could not be applied');
      return {
        errors: result.report.errors.slice(0, 10).map((e) => `${e.code} at ${e.path || '/'}: ${e.message}`),
        new_warnings: newIssues(before.warnings, result.report.warnings).slice(0, 10),
      };
    };

    if (ctx.autoAccept()) {
      const validation = apply(ops);
      return {
        result: { status: 'accepted', automatically: true, applied_ops: ops.length, validation, note: 'Applied without asking because the user turned on auto-accept. They can undo it.' },
        view: { title: 'Changed the plan', display: { type: 'change', proposal, state: 'auto' } },
      };
    }

    call.update({ title: 'Proposed changes, waiting for you', display: { type: 'change', proposal, state: 'pending' } });
    const decision = await ctx.requestApproval(call.id, proposal, call.signal);
    if (decision.type === 'decline') {
      return {
        result: {
          status: 'declined',
          ...(decision.reason ? { user_note: decision.reason } : {}),
          note: 'The user declined; the plan is unchanged. Do not propose the same edits again unless asked. Ask what they would prefer if it is not clear.',
        },
        view: { title: 'Proposed changes, declined', display: { type: 'change', proposal, state: 'declined' } },
      };
    }
    const final = decision.type === 'accept_edited' ? decision.ops : ops;
    let validation;
    try {
      validation = apply(final);
    } catch (error) {
      const message = error instanceof ToolError ? error.message : toEngineError(error).message;
      return {
        result: { status: 'failed', error: message, note: 'The user accepted, but the plan changed in the meantime and the edits no longer apply. Read the plan again and re-propose.' },
        isError: true,
        view: { title: 'Changes could not be applied', display: { type: 'change', proposal, state: 'failed' } },
      };
    }
    const edited = decision.type === 'accept_edited';
    return {
      result: {
        status: edited ? 'accepted_with_edits' : 'accepted',
        applied_ops: final.length,
        ...(edited ? { final_ops: final, note: 'The user changed some values before accepting. Their values are now in the plan and count as stated by them.' } : {}),
        validation,
      },
      view: {
        title: edited ? 'Changed the plan, with your edits' : 'Changed the plan',
        display: { type: 'change', proposal: edited ? { ...proposal, ops: final, entries: diffEntries(profile, final) } : proposal, state: edited ? 'edited' : 'accepted' },
      },
    };
  },
};
