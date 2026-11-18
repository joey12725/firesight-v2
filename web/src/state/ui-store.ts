import { create } from 'zustand';

export type ToastTone = 'neutral' | 'good' | 'warn' | 'bad';

export interface Toast {
  id: number;
  title: string;
  description?: string;
  tone: ToastTone;
  action?: { label: string; run: () => void };
  /** Milliseconds before it leaves; 0 keeps it until dismissed. */
  duration: number;
}

export type DialogName = 'settings' | 'share' | 'import' | 'setup' | null;
export type SheetName = 'editor' | 'assistant' | null;

interface UiState {
  dialog: DialogName;
  /** On narrow screens the editor and assistant open as sheets. */
  sheet: SheetName;
  toasts: Toast[];
  /** Editor section ids that are expanded. */
  openSections: string[];
  /** A field path to scroll to and focus, set by the plan check. */
  focusPath: string | null;
  /** Text handed to the assistant slot from elsewhere in the app (landing page). */
  assistantDraft: string | null;
  openDialog: (dialog: DialogName) => void;
  openSheet: (sheet: SheetName) => void;
  toggleSection: (id: string, open?: boolean) => void;
  setFocusPath: (path: string | null) => void;
  setAssistantDraft: (text: string | null) => void;
  toast: (toast: Omit<Partial<Toast>, 'id'> & { title: string }) => number;
  dismissToast: (id: number) => void;
}

let nextToast = 1;

export const useUi = create<UiState>((set) => ({
  dialog: null,
  sheet: null,
  toasts: [],
  openSections: ['household'],
  focusPath: null,
  assistantDraft: null,
  openDialog: (dialog) => set({ dialog }),
  openSheet: (sheet) => set({ sheet }),
  toggleSection: (id, open) =>
    set((s) => {
      const isOpen = s.openSections.includes(id);
      const want = open ?? !isOpen;
      if (want === isOpen) return s;
      return { openSections: want ? [...s.openSections, id] : s.openSections.filter((x) => x !== id) };
    }),
  setFocusPath: (focusPath) => set({ focusPath }),
  setAssistantDraft: (assistantDraft) => set({ assistantDraft }),
  toast: (toast) => {
    const id = nextToast++;
    set((s) => ({
      toasts: [...s.toasts.slice(-3), { tone: 'neutral', duration: toast.action ? 8000 : 4500, ...toast, id }],
    }));
    return id;
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

export const toast = (t: Parameters<UiState['toast']>[0]) => useUi.getState().toast(t);
