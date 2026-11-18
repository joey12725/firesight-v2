import { useState } from 'react';
import { applyPatch, defaultProfile, toEngineError } from '@/engine';
import type { PatchOp } from '@/engine/types';
import { US_STATES } from '@/editor/labels';
import { createHousehold } from '@/state/households';
import { append, set } from '@/state/patches';
import { useUi } from '@/state/ui-store';
import { Button, Dialog, Field, NumberInput, SegmentedControl, Select, Switch, TextInput } from '@/ui';

interface Answers {
  name: string;
  age: number;
  partner: boolean;
  partnerName: string;
  partnerAge: number;
  state: string;
  salary: number;
  partnerSalary: number;
  housing: number;
  rents: boolean;
  otherSpending: number;
  cash: number;
  pretax: number;
  roth: number;
  taxable: number;
  savingRate: number;
  matchRate: number;
  matchUpTo: number;
  goal: 'when_funded' | 'at_age';
  goalAge: number;
}

const INITIAL: Answers = {
  name: '',
  age: 35,
  partner: false,
  partnerName: '',
  partnerAge: 35,
  state: '',
  salary: 0,
  partnerSalary: 0,
  housing: 0,
  rents: true,
  otherSpending: 0,
  cash: 0,
  pretax: 0,
  roth: 0,
  taxable: 0,
  savingRate: 0.06,
  matchRate: 0.5,
  matchUpTo: 0.06,
  goal: 'when_funded',
  goalAge: 60,
};

/** Birth month for someone who is `age` today (born this month, `age` years ago). */
function birthFor(age: number): string {
  const now = new Date();
  return `${now.getFullYear() - Math.round(age)}-${String(now.getMonth() + 1).padStart(2, '0')}`;
}

/** The patch that turns the engine's starter profile into this household. */
export function buildSetupPatch(a: Answers): PatchOp[] {
  const you = a.name.trim() || 'You';
  const partner = a.partnerName.trim() || 'Partner';
  const plan = { employee_fraction: a.savingRate, match_rate: a.matchRate, match_up_to: a.matchUpTo };
  const ops: PatchOp[] = [
    set('/name', a.partner ? `${you} and ${partner}` : `${you}'s plan`),
    set('/household/people', [
      { id: 'you', name: you, birth: birthFor(a.age) },
      ...(a.partner ? [{ id: 'partner', name: partner, birth: birthFor(a.partnerAge) }] : []),
    ]),
    set('/household/location/state', a.state || null),
  ];
  if (a.salary > 0) ops.push(append('/incomes', { id: 'your_job', name: `${you}'s job`, owner: 0, kind: { type: 'job', salary: a.salary, plan: a.savingRate > 0 ? plan : null } }));
  if (a.partner && a.partnerSalary > 0) {
    ops.push(append('/incomes', { id: 'partner_job', name: `${partner}'s job`, owner: 1, kind: { type: 'job', salary: a.partnerSalary, plan: a.savingRate > 0 ? plan : null } }));
  }
  if (a.rents && a.housing > 0) ops.push(append('/spending/items', { id: 'rent', name: 'Rent', category: 'rent', amount: a.housing, frequency: 'monthly', essential: true }));
  if (!a.rents && a.housing > 0) {
    ops.push(append('/spending/items', { id: 'housing', name: 'Housing costs', category: 'other', amount: a.housing, frequency: 'monthly', essential: true }));
  }
  if (a.otherSpending > 0) {
    // A simple split the user can refine: 60% of everything else is treated as essential.
    ops.push(append('/spending/items', { id: 'essentials', name: 'Everyday essentials', category: 'groceries', amount: Math.round(a.otherSpending * 0.6), frequency: 'monthly', essential: true }));
    ops.push(append('/spending/items', { id: 'discretionary', name: 'Everything else', category: 'other', amount: Math.round(a.otherSpending * 0.4), frequency: 'monthly', essential: false }));
  }
  const account = (id: string, name: string, kind: string, balance: number) => balance > 0 && ops.push(append('/accounts', { id, name, owner: 0, kind, balance }));
  account('cash', 'Cash savings', 'cash', a.cash);
  account('pretax', '401(k) and traditional IRA', 'traditional', a.pretax);
  account('roth', 'Roth accounts', 'roth', a.roth);
  account('brokerage', 'Brokerage', 'taxable', a.taxable);
  ops.push(set('/retirement/trigger', a.goal === 'at_age' ? { type: 'at_age', person: 0, age: a.goalAge } : { type: 'when_funded' }));
  return ops;
}

const STEPS = ['You', 'Where you live', 'Income', 'Spending', 'Savings', 'Saving each year', 'The goal'] as const;

export function GuidedSetup() {
  const open = useUi((s) => s.dialog === 'setup');
  const close = () => useUi.getState().openDialog(null);
  const [step, setStep] = useState(0);
  const [a, setA] = useState<Answers>(INITIAL);
  const [error, setError] = useState<string | null>(null);
  const put = <K extends keyof Answers>(key: K, value: Answers[K]) => setA((prev) => ({ ...prev, [key]: value }));
  const num = (key: keyof Answers) => (v: number | null) => put(key, (v ?? 0) as never);
  const last = step === STEPS.length - 1;

  const finish = () => {
    try {
      const result = applyPatch(defaultProfile(), buildSetupPatch(a));
      createHousehold({ profile: result.profile, origin: 'setup' });
      close();
      setStep(0);
      setA(INITIAL);
      setError(null);
    } catch (e) {
      setError(toEngineError(e).message);
    }
  };

  const money = (key: keyof Answers, label: string, suffix: string, hint?: string) => (
    <Field label={label} hint={hint}>
      {({ id, describedBy }) => <NumberInput id={id} aria-describedby={describedBy} kind="money" min={0} suffix={suffix} value={a[key] as number} onChange={num(key)} />}
    </Field>
  );

  return (
    <Dialog
      open={open}
      onClose={close}
      title={STEPS[step] ?? ''}
      description={`Question ${step + 1} of ${STEPS.length}. Rough figures are fine; everything can be refined afterwards.`}
      footer={
        <>
          {step > 0 && (
            <Button variant="ghost" className="mr-auto" onClick={() => setStep(step - 1)}>
              Back
            </Button>
          )}
          <Button variant="primary" onClick={() => (last ? finish() : setStep(step + 1))}>
            {last ? 'Build my plan' : 'Continue'}
          </Button>
        </>
      }
    >
      <form
        className="space-y-4"
        onSubmit={(e) => {
          e.preventDefault();
          if (last) finish();
          else setStep(step + 1);
        }}
      >
        {step === 0 && (
          <>
            <div className="grid grid-cols-[1fr_7rem] gap-3">
              <Field label="Your first name">{({ id }) => <TextInput id={id} value={a.name} placeholder="Optional" onChange={(v) => put('name', v)} autoFocus />}</Field>
              <Field label="Age">{({ id }) => <NumberInput id={id} kind="integer" min={18} max={90} value={a.age} onChange={num('age')} />}</Field>
            </div>
            <Field inline label="Planning with a partner">
              {({ id }) => <Switch id={id} checked={a.partner} onChange={(v) => put('partner', v)} />}
            </Field>
            {a.partner && (
              <div className="grid grid-cols-[1fr_7rem] gap-3">
                <Field label="Their first name">{({ id }) => <TextInput id={id} value={a.partnerName} placeholder="Optional" onChange={(v) => put('partnerName', v)} />}</Field>
                <Field label="Age">{({ id }) => <NumberInput id={id} kind="integer" min={18} max={90} value={a.partnerAge} onChange={num('partnerAge')} />}</Field>
              </div>
            )}
          </>
        )}
        {step === 1 && (
          <Field label="State" hint="Sets state income tax and typical property tax. Living abroad can be set in the editor afterwards.">
            {({ id, describedBy }) => <Select id={id} aria-describedby={describedBy} value={a.state} placeholder="Choose a state" options={US_STATES} onChange={(v) => put('state', v)} />}
          </Field>
        )}
        {step === 2 && (
          <>
            {money('salary', a.partner ? 'Your pay before tax' : 'Pay before tax', '/yr', 'Salary only. Bonuses, equity and side income can be added later.')}
            {a.partner && money('partnerSalary', "Your partner's pay before tax", '/yr')}
          </>
        )}
        {step === 3 && (
          <>
            <Field label="Housing">
              {({ id }) => (
                <SegmentedControl
                  id={id}
                  aria-label="Housing"
                  fill
                  value={a.rents ? 'rent' : 'own'}
                  onChange={(v) => put('rents', v === 'rent')}
                  options={[
                    { value: 'rent', label: 'I rent' },
                    { value: 'own', label: 'I own' },
                  ]}
                />
              )}
            </Field>
            {money('housing', a.rents ? 'Rent' : 'Mortgage, tax and insurance', '/mo', a.rents ? undefined : 'Enter the home and mortgage themselves under Property afterwards for a proper model.')}
            {money('otherSpending', 'Everything else you spend', '/mo', 'Food, transport, insurance, fun, travel. Leave out tax and savings.')}
          </>
        )}
        {step === 4 && (
          <div className="grid grid-cols-2 gap-3">
            {money('cash', 'Cash and savings', '')}
            {money('pretax', '401(k), traditional IRA', '')}
            {money('roth', 'Roth accounts', '')}
            {money('taxable', 'Brokerage', '')}
          </div>
        )}
        {step === 5 && (
          <>
            <Field label="Share of pay going into a workplace plan" hint="Use 0 if there is no plan.">
              {({ id, describedBy }) => <NumberInput id={id} aria-describedby={describedBy} kind="percent" min={0} max={1} value={a.savingRate} onChange={num('savingRate')} />}
            </Field>
            {a.savingRate > 0 && (
              <div className="grid grid-cols-2 gap-3">
                <Field label="Employer matches">{({ id }) => <NumberInput id={id} kind="percent" min={0} value={a.matchRate} onChange={num('matchRate')} suffix="% of yours" />}</Field>
                <Field label="Up to">{({ id }) => <NumberInput id={id} kind="percent" min={0} value={a.matchUpTo} onChange={num('matchUpTo')} suffix="% of pay" />}</Field>
              </div>
            )}
          </>
        )}
        {step === 6 && (
          <>
            <Field label="When would you like to stop working?">
              {({ id }) => (
                <SegmentedControl
                  id={id}
                  aria-label="Retirement goal"
                  fill
                  value={a.goal}
                  onChange={(v) => put('goal', v)}
                  options={[
                    { value: 'when_funded', label: 'As soon as I can afford to' },
                    { value: 'at_age', label: 'At a particular age' },
                  ]}
                />
              )}
            </Field>
            {a.goal === 'at_age' ? (
              <Field label="Age">{({ id }) => <NumberInput id={id} kind="integer" min={a.age} max={80} value={a.goalAge} onChange={num('goalAge')} />}</Field>
            ) : (
              <p className="text-sm leading-relaxed text-ink-2">
                Each simulated life retires once its savings can cover your spending at a 4% withdrawal rate. The rate, and everything else, can be changed in the plan editor.
              </p>
            )}
            {error && (
              <p role="alert" className="text-sm text-bad">
                {error}
              </p>
            )}
          </>
        )}
        <button type="submit" className="sr-only" tabIndex={-1} aria-hidden>
          Continue
        </button>
      </form>
    </Dialog>
  );
}
