import { useEffect } from 'react';
import { loginPath, navigate, planPath } from '@/state/route';
import { toast } from '@/state/ui-store';
import { Skeleton } from '@/ui';
import { DEFAULT_VIEW } from '@/views/registry';
import { accountSync, useAccount } from './account';

/**
 * `/p/<plan id>`: the link an AI tool gives after writing a plan to the account. Signed
 * out, it goes by way of the login page and comes back. Signed in, the plan is opened
 * (fetched into this browser if it is not here yet) and the address is put right to
 * the ordinary one for the view.
 */
export function PlanLink({ id, view }: { id: string; view: string | null }) {
  const status = useAccount((s) => s.status);
  useEffect(() => {
    if (status === 'loading') return;
    if (status === 'off') {
      toast({ tone: 'warn', title: 'That link needs an account', description: 'This site has no accounts, so there is no plan at that address.' });
      navigate('/', { replace: true });
      return;
    }
    if (status === 'signed-out') {
      navigate(loginPath(planPath(id, view)), { replace: true });
      return;
    }
    let live = true;
    void accountSync()
      .then((sync) => {
        if (sync) return sync.openCloudPlan(id);
        toast({ tone: 'bad', title: 'Could not open that plan', description: 'Part of the app did not load. Check the connection and reload.' });
        return false;
      })
      .then((opened) => {
        if (live) navigate(opened && view && view !== DEFAULT_VIEW ? `/${view}` : '/', { replace: true });
      });
    return () => {
      live = false;
    };
  }, [status, id, view]);
  return (
    <div className="mx-auto w-full max-w-5xl space-y-4 px-8 pt-20" aria-busy="true" aria-label="Opening the plan from your account">
      <Skeleton className="h-3 w-24" />
      <Skeleton className="h-9 w-2/3" />
      <Skeleton className="h-5 w-1/2" />
    </div>
  );
}
