import { lazy, type ComponentType, type LazyExoticComponent } from 'react';
import type { ViewProps } from './types';

export interface ViewDefinition {
  /** URL segment: the view lives at `/<id>` (the first view also at `/`). */
  id: string;
  label: string;
  Component: LazyExoticComponent<ComponentType<ViewProps>>;
}

/** The tabs of the results workspace, in order. Each view is its own lazy chunk. */
export const VIEWS: ViewDefinition[] = [
  { id: 'overview', label: 'Overview', Component: lazy(() => import('./overview')) },
  { id: 'outcomes', label: 'Outcomes', Component: lazy(() => import('./outcomes')) },
  { id: 'cash-flow', label: 'Cash flow', Component: lazy(() => import('./cash-flow')) },
  { id: 'what-matters', label: 'What matters', Component: lazy(() => import('./what-matters')) },
  { id: 'scenarios', label: 'Scenarios', Component: lazy(() => import('./scenarios')) },
  { id: 'life-stories', label: 'Life stories', Component: lazy(() => import('./life-stories')) },
  { id: 'assumptions', label: 'Assumptions', Component: lazy(() => import('./assumptions')) },
];

export type ViewId = string;
export const DEFAULT_VIEW = 'overview';
export const isViewId = (id: string) => VIEWS.some((v) => v.id === id);
