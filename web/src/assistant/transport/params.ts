// The one place Messages API parameters are assembled, so both transports and the
// hosted proxy's validation agree on the request shape.
import type Anthropic from '@anthropic-ai/sdk';
import { BETA_CONTEXT_EDITING, BETA_FALLBACKS, MAX_OUTPUT_TOKENS, MODEL_CONFIG } from '../models';
import type { ModelRequest } from '../agent/types';

export type StreamParams = Anthropic.Beta.Messages.MessageCreateParamsNonStreaming;

/** Old tool results are cleared server-side once the prompt passes this size. */
const CLEAR_TOOL_RESULTS_AT = 100_000;

export function buildParams(request: ModelRequest): StreamParams {
  const config = MODEL_CONFIG[request.model];
  const betas: string[] = [BETA_CONTEXT_EDITING];
  if (config.fallbacks) betas.push(BETA_FALLBACKS);
  return {
    model: request.model,
    max_tokens: MAX_OUTPUT_TOKENS,
    // Render order is tools, system, messages. One explicit breakpoint at the end of
    // the frozen system prompt caches tools + system for every conversation; the
    // top-level marker below moves with the tail of this conversation.
    system: [{ type: 'text', text: request.system, cache_control: { type: 'ephemeral' } }],
    tools: request.tools,
    messages: request.messages,
    cache_control: { type: 'ephemeral' },
    // Thinking is adaptive on every model offered (budgets no longer exist); the
    // summary is requested so a long turn is not silent.
    thinking: { type: 'adaptive', display: 'summarized' },
    output_config: { effort: config.effort },
    // Stale tool results are pruned by the server; the client never edits history.
    context_management: {
      edits: [
        {
          type: 'clear_tool_uses_20250919',
          trigger: { type: 'input_tokens', value: CLEAR_TOOL_RESULTS_AT },
          keep: { type: 'tool_uses', value: 8 },
          clear_at_least: { type: 'input_tokens', value: 20_000 },
        },
      ],
    },
    // If a safety classifier declines a request, the API re-runs it on the model it
    // recommends for that category instead of failing the turn.
    ...(config.fallbacks ? { fallbacks: 'default' as const } : {}),
    betas,
  };
}
