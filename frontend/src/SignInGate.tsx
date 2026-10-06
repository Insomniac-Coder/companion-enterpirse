import { useEffect, useState } from 'react';
import App from './App';
import SignIn from './components/SignIn';
import { getMe, onSignInNeeded, type Me } from './services/account';

/** Asks the server who this is before the app makes any request. A server with sign-in gets
 * the sign-in page until the browser is signed in, and again when the session ends. An install
 * without sign-in always answers with the local person, so it goes straight to the app. */
export default function SignInGate() {
  const [state, setState] = useState<{ status: 'checking' | 'signed-out' | 'ready'; me?: Me; ended?: boolean }>({ status: 'checking' });
  useEffect(() => {
    let alive = true;
    getMe()
      .then((me) => { if (alive) setState(me ? { status: 'ready', me } : { status: 'signed-out' }); })
      // Server unreachable: the app shows that itself.
      .catch(() => { if (alive) setState({ status: 'ready' }); });
    const stop = onSignInNeeded(() => setState({ status: 'signed-out', ended: true }));
    return () => { alive = false; stop(); };
  }, []);
  if (state.status === 'checking') return null;
  if (state.status === 'signed-out') return <SignIn ended={!!state.ended} />;
  return <App me={state.me} />;
}
