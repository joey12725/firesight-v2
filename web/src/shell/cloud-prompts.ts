// Which question about a synced plan is on screen. Kept apart from the dialogs that
// answer it (cloud-ui.tsx) so that asking does not pull the sync code into the first load.
import { create } from 'zustand';

interface CloudPromptState {
  /** A question about one household's copy in the account. */
  prompt: { kind: 'delete' | 'stop-sync'; localId: string; name: string } | null;
  /** The conflict the person chose to decide later: `<local id>:<account revision>`. */
  postponed: string | null;
  ask: (prompt: CloudPromptState['prompt']) => void;
  reopenConflict: () => void;
}

export const useCloudUi = create<CloudPromptState>((set) => ({
  prompt: null,
  postponed: null,
  ask: (prompt) => set({ prompt }),
  reopenConflict: () => set({ postponed: null }),
}));
