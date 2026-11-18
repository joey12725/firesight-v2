import type { ViewProps } from '../types';
import { ViewPlaceholder } from '../ViewPlaceholder';

export default function OutcomesView(_props: ViewProps) {
  return (
    <ViewPlaceholder
      question="What could go wrong, when, and how bad would it be?"
      intro="Success or failure hides most of what matters. This view shows how lives end up, what the bad ones have in common, and how much spending was actually delivered."
      charts={[
        { title: 'Outcome at the end of life', shows: 'One hundred dots coloured by how each life finished: ran short, tight, comfortable, or a large surplus.' },
        { title: 'Spending delivered against plan', shows: 'Bands of real spending by age. Cuts made by a flexible spending rule are visible as the gap below the planned line.' },
        { title: 'What the short lives had in common', shows: 'Among lives that ran short: how many saw an early bear market, a job loss, a long-term-care event, a very long life or high inflation, with the depth and length of the shortfall.' },
        { title: 'Retirement age against spending', shows: 'A grid of retirement ages and spending levels, every cell simulated on the same futures, with contour lines where the plan stops holding.' },
      ]}
    />
  );
}
