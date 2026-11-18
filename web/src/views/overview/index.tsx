import type { ViewProps } from '../types';
import { ViewPlaceholder } from '../ViewPlaceholder';

export default function OverviewView(_props: ViewProps) {
  return (
    <ViewPlaceholder
      question="When can work become optional, and how sure is that?"
      intro="The first screen after any change: the age at which work becomes optional across every simulated life, and the range of wealth those lives pass through."
      charts={[
        { title: 'Age work becomes optional', shows: 'One hundred dots, each an equally likely life, placed at the age it reached independence. A toggle shows the same data as a cumulative curve: by this age in so many of 100 lives.' },
        { title: 'Outcome sentence', shows: 'The headline in counts rather than percentages (in 7 of 100 lives you run short), with its sampling interval and the same figure under each market model.' },
        { title: 'Net worth among the living', shows: "Bands in today's dollars at the 5th, 25th, 50th, 75th and 95th percentiles, with a few real simulated lives drawn through them and a strip showing how many are still alive." },
        { title: 'Life stages by age', shows: 'At each age, the share of lives still working, retired on plan, retired and cutting spending, short of essentials, or dead.' },
      ]}
    />
  );
}
