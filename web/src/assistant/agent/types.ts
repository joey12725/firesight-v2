// Shared types of the agent loop. API shapes come from the Anthropic SDK; nothing here
// redefines them.
import type Anthropic from '@anthropic-ai/sdk';
import type { AssistantModel } from '@/state/settings-store';

export type ApiMessage = Anthropic.Beta.Messages.BetaMessageParam;
export type ApiContent = Anthropic.Beta.Messages.BetaContentBlockParam;
export type ApiResponse = Anthropic.Beta.Messages.BetaMessage;
export type ApiEvent = Anthropic.Beta.Messages.BetaRawMessageStreamEvent;
export type ApiUsage = Anthropic.Beta.Messages.BetaUsage;
export type ApiTool = Anthropic.Beta.Messages.BetaTool;

/** What the loop asks a transport for. The transport turns it into API parameters. */
export interface ModelRequest {
  model: AssistantModel;
  /** Frozen for the life of a conversation: it is the head of the cached prefix. */
  system: string;
  /** Frozen and name-sorted, for the same reason. */
  tools: ApiTool[];
  messages: ApiMessage[];
}

/** The part of the SDK's message stream the loop uses; tests script it. */
export interface ModelStream extends AsyncIterable<ApiEvent> {
  finalMessage(): Promise<ApiResponse>;
  abort(): void;
}

export type TransportMode = 'byok' | 'hosted';

export interface Transport {
  mode: TransportMode;
  stream(request: ModelRequest, signal: AbortSignal): ModelStream;
}

export type AssistantErrorKind =
  | 'auth'
  | 'permission'
  | 'access_code'
  | 'rate_limit'
  | 'overloaded'
  | 'network'
  | 'too_large'
  | 'bad_request'
  | 'not_found'
  | 'server'
  | 'unavailable'
  | 'unknown';

/** An error in words a person can act on. */
export interface AssistantError {
  kind: AssistantErrorKind;
  title: string;
  message: string;
  /** The API's own message, for the curious. Never contains conversation content. */
  detail?: string;
  retryable: boolean;
  retryAfterMs?: number;
}

export interface ToolCall {
  id: string;
  name: string;
  input: unknown;
}

export interface ToolReply {
  /** Compact JSON text for the model. */
  content: string;
  isError: boolean;
}

export type TurnOutcome = 'complete' | 'aborted' | 'refusal' | 'max_tokens' | 'step_limit' | 'error';

export interface TurnUsage {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  /** Prompt size of the most recent request (input + cache read + cache write). */
  lastPrompt: number;
  requests: number;
}

export interface TurnResult {
  outcome: TurnOutcome;
  error?: AssistantError;
  /** Classifier category when `outcome` is `refusal`. */
  refusalCategory?: string | null;
  usage: TurnUsage;
}

export interface TurnEvents {
  /** A new request to the model begins (the first, or the one after tool results). */
  onStep?(step: number): void;
  onText?(delta: string): void;
  /** Summarised reasoning or a progress note between tool calls. */
  onThinking?(delta: string): void;
  /** The model has started writing a tool call; its input is not complete yet. */
  onToolDraft?(call: { id: string; name: string }): void;
  /** Complete tool calls of one response, about to run in parallel. */
  onToolCalls?(calls: ToolCall[]): void;
  onToolResult?(call: ToolCall, reply: ToolReply): void;
  /** The current step failed and will be sent again: drop what it streamed so far. */
  onRetry?(info: { attempt: number; delayMs: number; error: AssistantError }): void;
  onUsage?(usage: TurnUsage): void;
}
