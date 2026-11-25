import { lazy, type ComponentType, type LazyExoticComponent } from 'react';
import type { Profile } from '@/engine/types';
import { hasEquity } from './equity/model';
import type { ViewProps } from './types';

export interface ViewDefinition {
  /** URL segment: the view lives at `/<id>` (the first view also at `/`). */
  id: string;
  label: string;
  Component: LazyExoticComponent<ComponentType<ViewProps>>;
  /** Shown only for plans it has something to say about. A hidden view's id is still a valid address: it falls back to the first view. */
  when?: (profile: Profile) => boolean;
}

/** The tabs of the results workspace, in order. Each view is its own lazy chunk. */
export const VIEWS: ViewDefinition[] = [
  { id: 'overview', label: 'Overview', Component: lazy(() => import('./overview')) },
  { id: 'outcomes', label: 'Outcomes', Component: lazy(() => import('./outcomes')) },
  { id: 'cash-flow', label: 'Cash flow', Component: lazy(() => import('./cash-flow')) },
  // The same test as the Overview's card, so the card never links to a tab that is not there.
  { id: 'equity', label: 'Equity', when: hasEquity, Component: lazy(() => import('./equity')) },
  { id: 'what-matters', label: 'What matters', Component: lazy(() => import('./what-matters')) },
  { id: 'scenarios', label: 'Scenarios', Component: lazy(() => import('./scenarios')) },
  { id: 'life-stories', label: 'Life stories', Component: lazy(() => import('./life-stories')) },
  { id: 'assumptions', label: 'Assumptions', Component: lazy(() => import('./assumptions')) },
];

export type ViewId = string;
export const DEFAULT_VIEW = 'overview';
export const isViewId = (id: string) => VIEWS.some((v) => v.id === id);
/** The views this plan shows as tabs, in order. */
export const viewsFor = (profile: Profile | null): ViewDefinition[] => VIEWS.filter((v) => !v.when || (profile !== null && v.when(profile)));
