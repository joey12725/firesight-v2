// Keeping long conversations within budget without editing history.
//
// Current models bind every thinking block to the transcript that produced it, so the
// old habits (deleting stale tool results, keeping the last N turns verbatim) fail
// requests. Two mechanisms are used instead:
//   1. Server-side context editing clears old tool results (see transport/params.ts).
//      It does not count as an edit because the client keeps sending the full history.
//   2. Simple compaction, here: when the transcript is large, ask the model for a
//      summary and start a fresh transcript from it. Nothing from the old transcript
//      is replayed, so nothing can mismatch. It only ever runs between turns.
import { isAbortError } from './errors';
import type { ApiContent, ApiMessage, ModelRequest, Transport } from './types';

/** Compact when the last prompt was larger than this many tokens. */
export const COMPACT_AT_TOKENS = 150_000;
/** The hosted proxy sits behind a 4.5 MB request limit; compact well before it. */
export const HOSTED_COMPACT_AT_BYTES = 2_500_000;

const COMPACTION_PROMPT = `The conversation is getting long and will continue from a summary instead of this transcript. Write that summary now, as plain text, and do not call tools.

Keep, in this order:
1. What the user has told you about their household, goals and constraints, marking each item as stated by them, read from a document, or assumed by you.
2. Changes to the plan that were proposed, and for each whether the user accepted, edited or declined it.
3. Results that still matter: for each, the question, the figures with their intervals, and the run behind them (tool, paths, seed, scenario name). Drop superseded results.
4. Open questions and anything you promised to do next.

Do not include account numbers or identifiers. Figures in the summary will be treated as results from earlier in this conversation, so copy them exactly as the tools reported them.`;

/** Rough size of the transcript in tokens, for when no usage figure is available yet. */
export function estimateTokens(history: ApiMessage[]): number {
  let chars = 0;
  let media = 0;
  const visit = (block: ApiContent) => {
    if (block.type === 'image') media += 1_600;
    else if (block.type === 'document') media += block.source.type === 'base64' ? Math.ceil(block.source.data.length / 2_500) * 1_000 : 2_000;
    else if (block.type === 'tool_result') {
      if (typeof block.content === 'string') chars += block.content.length;
      else for (const inner of block.content ?? []) if (inner.type === 'text') chars += inner.text.length;
    } else chars += JSON.stringify(block).length;
  };
  for (const m of history) {
    if (typeof m.content === 'string') chars += m.content.length;
    else for (const block of m.content ?? []) visit(block);
  }
  return Math.ceil(chars / 3.5) + media;
}

export function historyBytes(history: ApiMessage[]): number {
  return JSON.stringify(history).length;
}

export function needsCompaction(history: ApiMessage[], lastPromptTokens: number, mode: Transport['mode'], limitTokens = COMPACT_AT_TOKENS): boolean {
  if (history.length < 4) return false;
  if (Math.max(lastPromptTokens, estimateTokens(history)) > limitTokens) return true;
  return mode === 'hosted' && historyBytes(history) > HOSTED_COMPACT_AT_BYTES;
}

export const SUMMARY_OPEN = '<conversation_summary>';
export const SUMMARY_CLOSE = '</conversation_summary>';

/**
 * Asks the model to summarise `history` and returns the opening block of the
 * transcript that replaces it, or null when no summary could be produced (the caller
 * then carries on with the full history). The request reuses the conversation's exact
 * system prompt, tools and messages, so it reads the prompt cache rather than paying
 * for the whole transcript again.
 */
export async function summariseHistory(
  transport: Transport,
  request: Omit<ModelRequest, 'messages'>,
  history: ApiMessage[],
  signal: AbortSignal,
): Promise<ApiContent | null> {
  const messages: ApiMessage[] = [...history, { role: 'user', content: [{ type: 'text', text: COMPACTION_PROMPT }] }];
  const stream = transport.stream({ ...request, messages }, signal);
  const onAbort = () => stream.abort();
  signal.addEventListener('abort', onAbort, { once: true });
  try {
    const message = await stream.finalMessage();
    if (message.stop_reason === 'refusal') return null;
    const text = message.content
      .flatMap((b) => (b.type === 'text' ? [b.text] : []))
      .join('\n')
      .trim();
    if (text.length < 40) return null;
    return {
      type: 'text',
      text: `${SUMMARY_OPEN}\nThe earlier part of this conversation was replaced by this summary, which you wrote.\n\n${text}\n${SUMMARY_CLOSE}`,
    };
  } catch (error) {
    if (signal.aborted || isAbortError(error)) throw error;
    return null;
  } finally {
    signal.removeEventListener('abort', onAbort);
  }
}
