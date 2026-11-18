// Model configuration for the assistant. Ids, thinking and effort follow the current
// Claude API guidance: every model here thinks adaptively (the `thinking` budget
// parameter no longer exists), depth is set with `output_config.effort`, and a
// classifier refusal can be re-run server-side on a fallback model where supported.
import { ASSISTANT_MODELS, type AssistantModel } from '@/state/settings-store';

export type Effort = 'low' | 'medium' | 'high';

export interface ModelConfig {
  /**
   * `medium` suits interactive analytical chat: the answer is grounded by tool calls,
   * not by long deliberation, and `high` makes every turn (greetings included) slow.
   * Pinned per model so the prompt cache is never invalidated by an effort change.
   */
  effort: Effort;
  /** Whether `fallbacks: "default"` (server-side refusal fallback) is offered for the model. */
  fallbacks: boolean;
}

export const MODEL_CONFIG: Record<AssistantModel, ModelConfig> = {
  'claude-fable-5-2': { effort: 'medium', fallbacks: true },
  'claude-opus-5-5': { effort: 'medium', fallbacks: true },
  'claude-sonnet-5-5': { effort: 'medium', fallbacks: true },
  'claude-haiku-5-5': { effort: 'medium', fallbacks: false },
};

/** The most capable current model; `useSettings` defaults to it. */
export const DEFAULT_MODEL: AssistantModel = 'claude-opus-5-5';

export const MODEL_IDS = ASSISTANT_MODELS.map((m) => m.id) as AssistantModel[];

export function isAssistantModel(id: string): id is AssistantModel {
  return (MODEL_IDS as string[]).includes(id);
}

export function modelLabel(id: string): string {
  return ASSISTANT_MODELS.find((m) => m.id === id)?.label ?? id;
}

/**
 * Output ceiling per response, thinking included. Requests are always streamed, so
 * this is not bounded by HTTP timeouts; the hosted proxy enforces the same cap.
 */
export const MAX_OUTPUT_TOKENS = 32_000;

/** Beta features the requests use. The hosted proxy forwards only these. */
export const BETA_CONTEXT_EDITING = 'context-management-2025-06-27';
export const BETA_FALLBACKS = 'server-side-fallback-2026-07-01';
