// The account pages: /login, /consent and /account. Loaded on demand, with the auth
// client, the first time one of them is opened.
import type { Page } from '@/state/route';
import { navigate } from '@/state/route';
import { Button, EmptyState, Skeleton } from '@/ui';
import { useAccount } from '../account';
import { Account } from './Account';
import { Consent } from './Consent';
import { Login } from './Login';
import { PageFrame } from './shared';

export default function AccountPages({ page }: { page: Page }) {
  const status = useAccount((s) => s.status);
  if (status === 'loading') {
    return (
      <PageFrame>
        <div className="space-y-4" aria-busy="true">
          <Skeleton className="h-8 w-2/3" />
          <Skeleton className="h-4 w-full" />
          <Skeleton className="h-4 w-5/6" />
        </div>
      </PageFrame>
    );
  }
  if (status === 'off') {
    return (
      <EmptyState icon="lock" title="This site has no accounts" action={<Button onClick={() => navigate('/', { replace: true })}>Back to FIRESight</Button>}>
        It keeps every plan in the browser it was made in and stores nothing on a server, so there is nothing to sign in to.
      </EmptyState>
    );
  }
  if (page.kind === 'login') return <Login />;
  if (page.kind === 'consent') return <Consent />;
  return <Account />;
}
