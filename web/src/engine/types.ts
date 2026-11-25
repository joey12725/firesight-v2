// The profile types the app works with.
//
// `profile.d.ts` is generated from the Rust schema, where every field has a default and
// is therefore optional. Profiles in the store have always been through the engine's
// typed round trip (`applyPatch`, `defaultProfile`, `migrateV1`), which writes every
// field, so the app sees the "complete" form: optional markers removed, nulls kept.
import type * as Gen from './profile';

/** Allocations are the one exception: the engine omits zero weights. */
export type AssetClass = keyof Gen.Allocation;
export type Allocation = Partial<Record<AssetClass, number>>;

type SameKeys<A, B> = [keyof A] extends [keyof B] ? ([keyof B] extends [keyof A] ? true : false) : false;

export type Complete<T> = T extends (infer U)[]
  ? Complete<U>[]
  : T extends object
    ? SameKeys<T, Gen.Allocation> extends true
      ? Allocation
      : { [K in keyof T]-?: Complete<T[K]> }
    : T;

export type Profile = Complete<Gen.Profile>;
export type Household = Profile['household'];
export type Person = Household['people'][number];
export type Child = Household['children'][number];
export type Location = Household['location'];
export type Income = Profile['incomes'][number];
export type IncomeKind = Income['kind'];
export type Job = Extract<IncomeKind, { type: 'job' }>;
export type EquityGrant = Job['equity'][number];
export type EmployerPlan = NonNullable<Job['plan']>;
export type PrivateCompany = Profile['private_equity'][number];
export type EquityHolding = PrivateCompany['holdings'][number];
export type HoldingKind = EquityHolding['kind'];
export type OutcomeRow = PrivateCompany['outcomes'][number];
export type OutcomeKind = OutcomeRow['kind'];
export type EquityPolicy = PrivateCompany['policy'];
export type CompanyStage = PrivateCompany['stage'];
export type When = Complete<Gen.When>;
export type Spending = Profile['spending'];
export type BudgetItem = Spending['items'][number];
export type Category = BudgetItem['category'];
export type Frequency = BudgetItem['frequency'];
export type Account = Profile['accounts'][number];
export type AccountKind = Account['kind'];
export type PortfolioPolicy = Profile['portfolio'];
export type GlidePath = PortfolioPolicy['glide'];
export type Property = Profile['properties'][number];
export type Mortgage = NonNullable<Property['mortgage']>;
export type Debt = Profile['debts'][number];
export type LifeEvent = Profile['events'][number];
export type EventKind = LifeEvent['kind'];
export type RetirementPlan = Profile['retirement'];
export type RetirementTrigger = RetirementPlan['trigger'];
export type SpendingRule = RetirementPlan['spending_rule'];
export type WithdrawalOrder = RetirementPlan['withdrawal_order'];
export type Assumptions = Profile['assumptions'];
export type Stress = Assumptions['market']['stress'][number];
export type SimulationSettings = Profile['simulation'];

/** One edit to a profile (core `patch.rs`). Paths are JSON pointers; an array segment may be `@<id>`. */
export type PatchOp =
  | { op: 'set'; path: string; value: unknown }
  | { op: 'remove'; path: string }
  | { op: 'append'; path: string; value: unknown };

/** One validation finding (core `validate.rs`). */
export interface Issue {
  code: string;
  path: string;
  message: string;
}

export interface Report {
  errors: Issue[];
  warnings: Issue[];
}

/** A sensitivity lever (core `scenario.rs`). */
export interface Lever {
  id: string;
  label: string;
  low_label: string;
  high_label: string;
  low: PatchOp[];
  high: PatchOp[];
}

export interface ExampleProfile {
  slug: string;
  title: string;
  summary: string;
  profile: Profile;
}
