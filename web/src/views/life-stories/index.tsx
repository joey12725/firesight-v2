import type { ViewProps } from '../types';
import { ViewPlaceholder } from '../ViewPlaceholder';

export default function LifeStoriesView(_props: ViewProps) {
  return (
    <ViewPlaceholder
      question="What does one of these lives actually look like?"
      intro="A percentile band is not a life. Here a single simulated life is told year by year, with the events that shaped it, picked by a stated rule and never presented as a forecast."
      charts={[
        { title: 'Annotated timeline', shows: 'Net worth and spending for one life, annotated from its ledger: a layoff during a recession, a market fall early in retirement, a spending cut, the move to Medicare, a care event.' },
        { title: 'Three named lives', shows: 'The median life, the life at the 10th percentile, and one that ran short if any did. Each states why it was chosen.' },
        { title: 'Another life', shows: 'A button that draws one more random life, one at a time and at your pace. Nothing plays by itself.' },
        { title: 'Same life, other plan', shows: 'Because path numbers mean the same world everywhere, any life can be replayed under a scenario to see what the change would have done in that exact future.' },
      ]}
    />
  );
}
