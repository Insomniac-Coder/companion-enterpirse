import { signInUrl } from '../services/account';
import { Button } from '../ui/primitives';

/** The page a server with sign-in shows to a browser that is not signed in. */
export default function SignIn({ ended }: { ended: boolean }) {
  const signIn = () => { window.location.href = signInUrl(window.location.pathname + window.location.search); };
  return (
    <main className="sign-in">
      <div className="sign-in-card">
        <strong className="sign-in-name">Companion</strong>
        <h1>{ended ? 'Your sign-in has ended' : 'Sign in to Companion'}</h1>
        <p>{ended ? 'Sign in again to carry on. Your conversations are kept.' : 'Use your company account. You will come straight back here.'}</p>
        <Button variant="primary" size="lg" icon="lock" onClick={signIn}>Sign in</Button>
      </div>
    </main>
  );
}
