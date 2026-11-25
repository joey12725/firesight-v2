// Takes pending cards down when what they were raised for no longer holds. A card shows
// a change to one plan, with that plan's before and after: once another plan is opened
// (by the user, by account sync, or by the agent's own open_demo) accepting it would
// write to the wrong plan. And "Off" means off: nothing an agent asked for earlier may
// still be accepted afterwards.
import { useProfileStore } from '@/state/profile-store';
import { useSettings } from '@/state/settings-store';
import { withdrawApprovals } from './agent-store';

let stop: (() => void) | null = null;

/** Starts watching (once, however often it is called). Returns a function that stops it. */
export function watchApprovals(): () => void {
  if (!stop) {
    // `meta` is replaced exactly when a plan is loaded or closed, including a reload of the same plan.
    const plan = useProfileStore.subscribe((s, p) => {
      if (s.meta !== p.meta) withdrawApprovals();
    });
    const access = useSettings.subscribe((s, p) => {
      if (s.agentAccess === 'off' && p.agentAccess !== 'off') withdrawApprovals();
    });
    const mine = () => {
      plan();
      access();
      if (stop === mine) stop = null;
    };
    stop = mine;
  }
  return stop;
}
