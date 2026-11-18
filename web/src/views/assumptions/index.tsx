import type { ViewProps } from '../types';
import { ViewPlaceholder } from '../ViewPlaceholder';

export default function AssumptionsView(_props: ViewProps) {
  return (
    <ViewPlaceholder
      question="What is this model assuming, and how much does that matter?"
      intro="Every answer here rests on a model of markets, inflation, taxes and lifespans. This view lays those assumptions out and shows how far the headline moves when they change."
      charts={[
        { title: 'Three market models, one plan', shows: 'The headline figure with its interval under history reshuffled, the regime model and the textbook random walk, side by side.' },
        { title: 'Simulated returns against history', shows: 'The distribution of long-run stock, bond and inflation outcomes the model generates, set against what actually happened.' },
        { title: 'Lifespans', shows: 'Survival curves for each person and for the last survivor, from the life tables with your health settings applied.' },
        { title: 'Tax and benefit parameters', shows: 'The brackets, limits and benefit formulas in force, with the year and source for each.' },
      ]}
    />
  );
}
