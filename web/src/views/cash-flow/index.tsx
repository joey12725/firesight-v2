import type { ViewProps } from '../types';
import { ViewPlaceholder } from '../ViewPlaceholder';

export default function CashFlowView(_props: ViewProps) {
  return (
    <ViewPlaceholder
      question="Where does the money come from, and where does it go?"
      intro="Income, tax, spending and saving year by year, for one named life at a time, so every figure can be traced to its ledger."
      charts={[
        { title: 'One year of money', shows: 'A flow diagram for a single year of the selected life: wages, benefits and withdrawals on the left; taxes, healthcare, housing, spending and saving on the right. A scrubber moves through the years.' },
        { title: 'Accounts over time', shows: 'Balances by tax treatment (cash, taxable, pre-tax, Roth, HSA, home equity) for the selected life, against the median across all lives.' },
        { title: 'Tax bracket fill', shows: "Each year's taxable income by source, stacked against bracket thresholds in today's dollars, with the health-subsidy, Medicare surcharge and 0% capital-gains limits marked." },
        { title: 'Year ledger', shows: 'The full table behind any year: every income, tax line, withdrawal and balance.' },
      ]}
    />
  );
}
