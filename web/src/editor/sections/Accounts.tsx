import type { Account, AccountKind, PortfolioPolicy } from '@/engine/types';
import { set } from '@/state/patches';
import { at, join } from '@/state/pointer';
import { Disclosure, Field, Switch, moneyCompact } from '@/ui';
import { AllocationEditor } from '../AllocationEditor';
import { ChoiceField, Group, MoneyField, NumField, OwnerField, PercentField, Realism, Row, SegmentField, SliderField, TextField, ToggleField } from '../fields';
import { useEdit, useHelp, useProfile } from '../hooks';
import { ACCOUNT_KINDS } from '../labels';
import { portfolioRealism } from '../realism';
import { AddButton, Item } from '../Section';
import { useAdd } from '../useAdd';

function AccountItem({ account, index, names }: { account: Account; index: number; names: string[] }) {
  const path = join('/accounts', at(account, index));
  const edit = useEdit();
  const holdingsHelp = useHelp(`${path}/holdings`);
  return (
    <Item path={path} figure={moneyCompact(account.balance)} detail={names.length > 1 ? names[account.owner] : undefined} fallbackName="Account">
      <Row>
        <MoneyField path={`${path}/balance`} label="Balance" />
        <ChoiceField path={`${path}/kind`} label="Type" options={ACCOUNT_KINDS} />
      </Row>
      <OwnerField path={`${path}/owner`} />
      {account.kind === 'taxable' && <MoneyField path={`${path}/cost_basis`} label="Cost basis" nullable placeholder="60% of balance" hint="What you paid for the holdings. The rest is taxable gain." />}
      {account.kind === 'roth' && <MoneyField path={`${path}/roth_basis`} label="Contributions (basis)" nullable placeholder="50% of balance" hint="Withdrawable any time without tax or penalty." />}
      {account.kind === 'cash' && (
        <Row>
          <ChoiceField
            path={`${path}/cash_yield`}
            label="Interest"
            options={[
              { value: 'tracks_bills', label: 'Tracks short-term rates' },
              { value: 'fixed', label: 'Fixed rate' },
            ]}
            read={(v) => (v as Account['cash_yield']).type}
            write={(type) => (type === 'fixed' ? { type, rate: 0 } : { type })}
          />
          {account.cash_yield.type === 'fixed' ? <PercentField path={`${path}/cash_yield/rate`} label="Rate" /> : <PercentField path={`${path}/cash_yield/spread`} label="Spread to T-bills" />}
        </Row>
      )}
      {account.kind !== 'cash' && (
        <div className="space-y-3">
          <Field inline label="Holds a different mix from the target" help={holdingsHelp}>
            {({ id }) => <Switch id={id} checked={account.holdings !== null} onChange={(on) => edit(set(`${path}/holdings`, on ? { us_stocks: 100 } : null))} />}
          </Field>
          {account.holdings !== null && <AllocationEditor path={`${path}/holdings`} label="Current holdings" />}
        </div>
      )}
    </Item>
  );
}

const GLIDE = [
  { value: 'none', label: 'Fixed mix' },
  { value: 'target_date', label: 'Target-date glide' },
  { value: 'bond_tent', label: 'Bond tent' },
  { value: 'custom', label: 'Custom schedule' },
] as const;

function GlideFields({ glide }: { glide: PortfolioPolicy['glide'] }) {
  const p = '/portfolio/glide';
  const add = useEdit();
  return (
    <div className="space-y-3">
      <ChoiceField
        path={p}
        label="Glide path"
        options={GLIDE}
        read={(v) => (v as PortfolioPolicy['glide']).type}
        write={(type) => (type === 'custom' ? { type, points: [{ years_to_retirement: 20, equity_fraction: 0.9 }, { years_to_retirement: 0, equity_fraction: 0.6 }] } : { type })}
      />
      {(glide.type === 'target_date' || glide.type === 'bond_tent') && (
        <>
          <Row>
            <NumField path={`${p}/years_before`} label="Starts before retirement" kind="years" suffix="yrs" />
            <PercentField path={`${p}/equity_at_retirement`} label="Growth assets at retirement" />
          </Row>
          <Row>
            <NumField path={`${p}/years_after`} label="Continues after" kind="years" suffix="yrs" />
            {glide.type === 'target_date' && <PercentField path={`${p}/equity_final`} label="Growth assets at the end" />}
          </Row>
        </>
      )}
      {glide.type === 'custom' && (
        <div className="space-y-1.5">
          {glide.points.map((_, i) => (
            <Row key={i}>
              <NumField path={`${p}/points/${i}/years_to_retirement`} label={i === 0 ? 'Years to retirement' : ''} kind="years" suffix="yrs" />
              <PercentField path={`${p}/points/${i}/equity_fraction`} label={i === 0 ? 'Growth assets' : ''} />
            </Row>
          ))}
          <AddButton label="Add a point" onAdd={() => add({ op: 'append', path: `${p}/points`, value: { years_to_retirement: -10, equity_fraction: 0.5 } })} />
        </div>
      )}
    </div>
  );
}

function PortfolioFields({ portfolio }: { portfolio: PortfolioPolicy }) {
  const edit = useEdit();
  const customHelp = useHelp('/portfolio/custom_asset');
  const hasSingle = (portfolio.target.single_stock ?? 0) > 0;
  const hasCustom = (portfolio.target.custom ?? 0) > 0;
  return (
    <Group title="Investment plan">
      <AllocationEditor path="/portfolio/target" label="Target allocation" />
      <GlideFields glide={portfolio.glide} />
      <Row>
        <ChoiceField
          path="/portfolio/rebalance"
          label="Rebalancing"
          options={[
            { value: 'annual', label: 'Once a year' },
            { value: 'bands', label: 'When it drifts' },
            { value: 'never', label: 'Never' },
          ]}
          read={(v) => (v as PortfolioPolicy['rebalance']).type}
          write={(type) => (type === 'bands' ? { type, width: 0.05 } : { type })}
        />
        {portfolio.rebalance.type === 'bands' ? (
          <PercentField path="/portfolio/rebalance/width" label="Drift allowed" />
        ) : (
          <SegmentField
            path="/portfolio/location"
            label="Asset location"
            options={[
              { value: 'mirror', label: 'Same everywhere' },
              { value: 'tax_efficient', label: 'Tax-efficient' },
            ]}
          />
        )}
      </Row>
      {portfolio.rebalance.type === 'bands' && (
        <SegmentField
          path="/portfolio/location"
          label="Asset location"
          options={[
            { value: 'mirror', label: 'Same mix in every account' },
            { value: 'tax_efficient', label: 'Tax-efficient placement' },
          ]}
        />
      )}
      <Row>
        <PercentField path="/portfolio/expense_ratio" label="Fund costs" step={0.0005} />
        <PercentField path="/portfolio/advisory_fee" label="Adviser fee" step={0.0025} />
      </Row>
      <SliderField path="/portfolio/emergency_months" label="Emergency fund" min={0} max={24} step={1} suffix="months" hint="Held in cash, sized to current spending." />
      <SliderField path="/portfolio/retirement_cash_years" label="Cash bucket in retirement" min={0} max={5} step={0.5} suffix="years" hint="Spending kept in cash so stocks are not sold in a downturn." />

      {(hasSingle || hasCustom || portfolio.custom_asset) && (
        <Disclosure summary="Single stock and custom asset" defaultOpen>
          <div className="space-y-3">
            {hasSingle && (
              <Row>
                <PercentField path="/portfolio/single_stock/volatility" label="Single stock volatility" />
                <NumField path="/portfolio/single_stock/beta" label="Single stock beta" step={0.1} />
              </Row>
            )}
            <Field inline label="Define a custom asset" help={customHelp}>
              {({ id }) => (
                <Switch
                  id={id}
                  checked={portfolio.custom_asset !== null}
                  onChange={(on) => edit(set('/portfolio/custom_asset', on ? { name: 'Custom asset', mean: 0.05, volatility: 0.15, equity_corr: 0.3 } : null))}
                />
              )}
            </Field>
            {portfolio.custom_asset && (
              <>
                <TextField path="/portfolio/custom_asset/name" label="Name" />
                <Row cols={3}>
                  <PercentField path="/portfolio/custom_asset/mean" label="Return" />
                  <PercentField path="/portfolio/custom_asset/volatility" label="Volatility" />
                  <NumField path="/portfolio/custom_asset/equity_corr" label="Stock corr." step={0.1} />
                </Row>
                <PercentField path="/portfolio/custom_asset/income_yield" label="Paid out as income each year" hint="Part of the return above, taxed as ordinary income in a taxable account. 0 if it pays nothing." />
              </>
            )}
          </div>
        </Disclosure>
      )}
      <Realism summary="Tax-loss harvesting, panic selling and pausing saving" on={portfolioRealism(portfolio)}>
        <ToggleField path="/portfolio/tax_loss_harvesting" label="Harvest tax losses in the taxable account" />
        <ChoiceField
          path="/portfolio/behavior"
          label="In a crash"
          options={[
            { value: 'disciplined', label: 'Sticks to the plan' },
            { value: 'panic_prone', label: 'May sell stocks in a panic' },
          ]}
          read={(v) => (v as PortfolioPolicy['behavior']).type}
          write={(type) => ({ type })}
        />
        {portfolio.behavior.type === 'panic_prone' && (
          <>
            <Row cols={3}>
              <PercentField path="/portfolio/behavior/drawdown_trigger" label="After a fall of" />
              <PercentField path="/portfolio/behavior/probability" label="Chance" />
              <PercentField path="/portfolio/behavior/share" label="Stocks sold" />
            </Row>
            <Row>
              <PercentField path="/portfolio/behavior/reentry/recovered_within" label="Buys back within, of the peak" />
              <NumField path="/portfolio/behavior/reentry/max_months" label="Or after" kind="months" suffix="mo" />
            </Row>
          </>
        )}
        <ToggleField path="/portfolio/pause_deferrals_in_distress" label="Pause retirement saving when money is tight" />
        {portfolio.pause_deferrals_in_distress && (
          <Row>
            <ToggleField path="/portfolio/distress/keep_match" label="Keep the employer match" />
            <PercentField path="/portfolio/distress/high_interest_rate" label="Debt counts as costly from" />
          </Row>
        )}
      </Realism>
    </Group>
  );
}

export function AccountsSection() {
  const profile = useProfile();
  const add = useAdd('/accounts');
  const names = profile.household.people.map((p) => p.name);
  const groups = ACCOUNT_KINDS.map((k) => ({
    ...k,
    accounts: profile.accounts.map((account, index) => ({ account, index })).filter((x) => x.account.kind === k.value),
  })).filter((g) => g.accounts.length > 0);
  const addAccount = (kind: AccountKind, name: string) => add(name, { kind, balance: 0 });

  return (
    <>
      <div className="space-y-4">
        {groups.map((g) => (
          <div key={g.value}>
            <div className="mb-1.5 flex items-baseline justify-between">
              <h4 className="eyebrow">{g.plural}</h4>
              <span className="num text-xs text-ink-3">{moneyCompact(g.accounts.reduce((n, x) => n + x.account.balance, 0))}</span>
            </div>
            <ul className="space-y-1.5">
              {g.accounts.map(({ account, index }) => (
                <AccountItem key={account.id || index} account={account} index={index} names={names} />
              ))}
            </ul>
          </div>
        ))}
        {profile.accounts.length === 0 && <p className="text-xs text-ink-3">No accounts yet. Add what you hold today, grouped by how it is taxed.</p>}
        <div className="flex flex-wrap gap-x-1">
          <AddButton label="Cash" onAdd={() => addAccount('cash', 'Savings')} />
          <AddButton label="Taxable" onAdd={() => addAccount('taxable', 'Brokerage')} />
          <AddButton label="Pre-tax" onAdd={() => addAccount('traditional', '401(k)')} />
          <AddButton label="Roth" onAdd={() => addAccount('roth', 'Roth IRA')} />
          <AddButton label="HSA" onAdd={() => addAccount('hsa', 'HSA')} />
        </div>
      </div>
      <PortfolioFields portfolio={profile.portfolio} />
    </>
  );
}
