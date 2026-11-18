// The conversation as the UI shows it, and its storage. One conversation per household,
// kept in this browser's IndexedDB next to the household itself.
import { createStore, del, get, set } from 'idb-keyval';
import type { ApiMessage, AssistantError, TransportMode } from './agent/types';
import type { ToolDisplay, ToolView } from './tools/types';

export interface AttachmentInfo {
  name: string;
  mediaType: string;
  bytes: number;
}

export type ToolStatus = 'drafting' | 'running' | 'done' | 'error' | 'cancelled';

export interface TextPart {
  type: 'text';
  text: string;
}

export interface ThinkingPart {
  type: 'thinking';
  text: string;
}

export interface ToolPart {
  type: 'tool';
  id: string;
  name: string;
  input: unknown;
  status: ToolStatus;
  title: string;
  /** What the model received, for the expandable detail. */
  result?: string;
  meta?: ToolView['meta'];
  display?: ToolDisplay;
}

export type Part = TextPart | ThinkingPart | ToolPart;

export interface UserItem {
  kind: 'user';
  id: string;
  text: string;
  attachments: AttachmentInfo[];
  /** Position of this turn's user message in the API transcript; -1 once it has been summarised away. */
  apiIndex: number;
}

export type AssistantStatus = 'streaming' | 'done' | 'stopped' | 'error' | 'refused' | 'truncated' | 'step_limit';

export interface AssistantItem {
  kind: 'assistant';
  id: string;
  parts: Part[];
  status: AssistantStatus;
  followups: string[];
  error?: AssistantError;
  model: string;
  mode: TransportMode;
}

export interface NoteItem {
  kind: 'note';
  id: string;
  text: string;
}

export type ChatItem = UserItem | AssistantItem | NoteItem;

export interface StoredConversation {
  version: 1;
  items: ChatItem[];
  /** The exact transcript sent to the API. Append-only between compactions. */
  history: ApiMessage[];
  /** Prompt + tools the transcript was produced under. */
  fingerprint: string;
  /** Prompt size of the last request, for deciding when to compact. */
  lastPrompt: number;
  /** Hash of the plan state the model last saw. */
  planKey: string | null;
  /** A summary waiting to open the next transcript. */
  pendingSummary: string | null;
}

export interface ConversationStorage {
  load(householdId: string): Promise<StoredConversation | undefined>;
  save(householdId: string, conversation: StoredConversation): Promise<void>;
  remove(householdId: string): Promise<void>;
}

// Created lazily: IndexedDB is unavailable in some private modes, and callers handle that.
let store: ReturnType<typeof createStore> | null = null;
const db = () => (store ??= createStore('firesight-assistant', 'conversations'));

export const browserStorage: ConversationStorage = {
  load: async (id) => {
    const found = await get<StoredConversation>(id, db());
    return found?.version === 1 ? found : undefined;
  },
  save: (id, conversation) => set(id, conversation, db()),
  remove: (id) => del(id, db()),
};

/** Removes a household's conversation, for when the household itself is deleted. */
export function deleteConversation(householdId: string): Promise<void> {
  return browserStorage.remove(householdId).catch(() => undefined);
}

/**
 * Makes a stored conversation safe to continue: nothing is mid-flight any more, and the
 * transcript ends where the API can pick it up (no tool call left without its result).
 */
export function settle(conversation: StoredConversation): StoredConversation {
  const items = conversation.items.map((item): ChatItem => {
    if (item.kind !== 'assistant') return item;
    return {
      ...item,
      status: item.status === 'streaming' ? 'stopped' : item.status,
      parts: item.parts.map((part): Part => {
        if (part.type !== 'tool') return part;
        const interrupted = part.status === 'drafting' || part.status === 'running';
        const display = part.display?.type === 'change' && part.display.state === 'pending' ? { ...part.display, state: 'cancelled' as const } : part.display;
        return { ...part, status: interrupted ? 'cancelled' : part.status, display };
      }),
    };
  });
  const history = [...conversation.history];
  trimDangling(history);
  return { ...conversation, items, history };
}

/** Drops a trailing assistant message whose tool calls never got their results. Cutting the end of a transcript is always safe. */
export function trimDangling(history: ApiMessage[]): void {
  for (;;) {
    const last = history[history.length - 1];
    if (last?.role === 'assistant' && Array.isArray(last.content) && last.content.some((b) => b.type === 'tool_use')) history.pop();
    else break;
  }
}
