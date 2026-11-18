import type { ViewProps } from '../types';
import { ViewPlaceholder } from '../ViewPlaceholder';
import { ScenarioList } from './ScenarioList';

export default function ScenariosView(_props: ViewProps) {
  return (
    <>
      <ScenarioList />
      <ViewPlaceholder
        question="What should I change, and does it really help?"
        intro="Every scenario lives through exactly the same simulated futures as your baseline, so a difference between them is the change itself and never market luck."
        charts={[
          { title: 'Rescued, harmed, unchanged', shows: 'Of every 1,000 lives: how many the change rescues from running short, how many it harms, and how many end the same, always with the interval.' },
          { title: 'Difference, life by life', shows: 'One hundred dots of the per-life difference in lifetime spending or wealth at 85, with zero marked.' },
          { title: 'Side by side', shows: 'The same band chart for up to four scenarios on shared axes, with the same three figures under each.' },
        ]}
      />
    </>
  );
}
