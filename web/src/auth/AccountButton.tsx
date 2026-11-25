import { activePage, loginPath, navigate } from '@/state/route';
import { Button, IconButton } from '@/ui';
import { useAccount } from './account';

/** Top bar: "Sign in", or the way to the account page. Nothing at all on a site without accounts. */
export function AccountButton() {
  const status = useAccount((s) => s.status);
  const user = useAccount((s) => s.user);
  if (status === 'signed-in') return <IconButton icon="user" label="Your account" hint={user?.email} data-testid="account-button" onClick={() => navigate('/account')} />;
  if (status !== 'signed-out') return null;
  return (
    <Button
      size="sm"
      variant="ghost"
      data-testid="sign-in"
      onClick={() => {
        // Back to where they were, unless that is itself an account page.
        const here = activePage().kind === 'app' ? window.location.pathname + window.location.search : '/';
        navigate(loginPath(here));
      }}
    >
      Sign in
    </Button>
  );
}
