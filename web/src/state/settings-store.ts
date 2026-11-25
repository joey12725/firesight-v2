import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export type ThemeChoice = 'system' | 'light' | 'dark';

/**
 * Models offered for the assistant: the current Claude family, most capable first.
 * The hosted proxy (api/chat.ts) keeps the same allow-list; a test compares them.
 */
export const ASSISTANT_MODELS = [
  { id: 'claude-fable-5-2', label: 'Claude Fable 5.2', note: 'Most capable, highest cost. The API account needs 30-day data retention enabled.' },
  { id: 'claude-opus-5-5', label: 'Claude Opus 5.5', note: 'Half the cost of Fable; strong for planning conversations' },
  { id: 'claude-sonnet-5-5', label: 'Claude Sonnet 5.5', note: 'Faster and cheaper' },
  { id: 'claude-haiku-5-5', label: 'Claude Haiku 5.5', note: 'Fastest, lowest cost' },
] as const;

export type AssistantModel = (typeof ASSISTANT_MODELS)[number]['id'];

interface SettingsState {
  theme: ThemeChoice;
  /** Bring-your-own Claude API key. Lives only in this browser's localStorage. */
  apiKey: string;
  model: AssistantModel;
  /** Apply the assistant's proposed plan changes without asking. Each one can still be undone. */
  assistantAutoAccept: boolean;
  /** Shared code for the site's hosted assistant, when the operator requires one. */
  hostedAccessCode: string;
  editorOpen: boolean;
  assistantOpen: boolean;
  lastHouseholdId: string | null;
  /**
   * Whether an AI agent in this browser may call FIRESight's in-page tools (WebMCP).
   * `ask`: the page asks once per session. `allow`: no prompt. `off`: the tools refuse.
   */
  agentAccess: 'ask' | 'allow' | 'off';
  /** Apply a browser agent's plan changes without a change card. Each one can still be undone. */
  agentAutoApply: boolean;
  set: (patch: Partial<Omit<SettingsState, 'set'>>) => void;
}

export const useSettings = create<SettingsState>()(
  persist(
    (set) => ({
      theme: 'system',
      apiKey: '',
      model: 'claude-opus-5-5',
      assistantAutoAccept: false,
      hostedAccessCode: '',
      editorOpen: true,
      assistantOpen: true,
      lastHouseholdId: null,
      agentAccess: 'ask',
      agentAutoApply: false,
      set: (patch) => set(patch),
    }),
    { name: 'firesight.settings', version: 1 },
  ),
);

export function resolveTheme(choice: ThemeChoice): 'light' | 'dark' {
  if (choice !== 'system') return choice;
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

/** Keeps `<html data-theme>` in step with the setting and the OS preference. */
export function startThemeSync() {
  const apply = () => {
    document.documentElement.dataset.theme = resolveTheme(useSettings.getState().theme);
    window.dispatchEvent(new Event('firesight:theme'));
  };
  apply();
  useSettings.subscribe((state, previous) => {
    if (state.theme !== previous.theme) apply();
  });
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', apply);
}
