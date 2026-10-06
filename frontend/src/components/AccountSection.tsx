import { useEffect, useState } from 'react';
import { apiKeyNameProblem, createApiKey, listApiKeys, revokeApiKey, signOut, type ApiKey, type Me } from '../services/account';
import { Button } from '../ui/primitives';
import { Icon } from '../ui/Icon';
import { pushToast, type Toast } from './Toasts';

const when = (at: string | null) => (at ? new Date(at).toLocaleString() : 'never');

/** Settings > Account, on a server with sign-in: who is signed in, signing out, and API keys. */
export default function AccountSection({ me, setToasts }: { me: Me; setToasts: React.Dispatch<React.SetStateAction<Toast[]>> }) {
  const [keys, setKeys] = useState<ApiKey[] | null>(null);
  const [name, setName] = useState('');
  const [made, setMade] = useState<{ name: string; key: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const fail = (error: unknown) => pushToast(setToasts, 'error', error instanceof Error ? error.message : 'That did not work.');
  const refresh = () => listApiKeys().then(setKeys).catch(fail);
  useEffect(() => { void refresh(); }, []);

  const problem = name ? apiKeyNameProblem(name) : null;
  const make = async () => {
    if (apiKeyNameProblem(name)) return;
    setBusy(true);
    try {
      const key = await createApiKey(name);
      setMade({ name: key.name, key: key.key });
      setName('');
      await refresh();
    } catch (error) { fail(error); } finally { setBusy(false); }
  };
  const withdraw = async (key: ApiKey) => {
    if (!window.confirm(`Withdraw the key "${key.name}"? Software using it stops working at once.`)) return;
    try {
      await revokeApiKey(key.id);
      if (made?.name === key.name) setMade(null);
      await refresh();
    } catch (error) { fail(error); }
  };
  const copy = async (text: string) => {
    try { await navigator.clipboard.writeText(text); pushToast(setToasts, 'success', 'Key copied.'); } catch { pushToast(setToasts, 'error', 'Copy it from the box instead.'); }
  };

  return (
    <section className="settings-section account" data-settings-title="Account">
      <h2><Icon name="lock" size={16} />Account</h2>
      <p className="settings-section-intro">Signed in as <strong>{me.name}</strong>{me.email && me.email !== me.name ? ` (${me.email})` : ''}. Changes here apply at once.</p>
      <Button size="sm" icon="power" onClick={() => { void signOut().then(() => { window.location.href = '/'; }, fail); }}>Sign out</Button>

      <h3 className="account-heading">API keys</h3>
      <p className="settings-capability-note">For software that works for you, such as a build server or a script. It sends the key as <code>Authorization: Bearer &lt;key&gt;</code> and can do what you can. Make one key per program, so each can be withdrawn on its own.</p>
      <form className="account-new-key" onSubmit={(event) => { event.preventDefault(); void make(); }}>
        <input aria-label="Name for a new key" placeholder="Name, e.g. Build server" value={name} maxLength={120} onChange={(event) => setName(event.target.value)} aria-invalid={!!problem} aria-describedby="account-key-problem" />
        <Button type="submit" variant="primary" size="sm" icon="plus" loading={busy} disabled={!!apiKeyNameProblem(name)}>Make key</Button>
      </form>
      {problem && <p id="account-key-problem" className="settings-context-warning" role="alert">{problem}</p>}
      {made && (
        <div className="account-made" role="status">
          <p><strong>{made.name}</strong>: copy the key now. It is not shown again.</p>
          <div className="account-made-key"><code>{made.key}</code><Button size="sm" icon="copy" onClick={() => void copy(made.key)}>Copy</Button></div>
        </div>
      )}
      {keys === null ? <p className="settings-capability-note" role="status">Loading keys…</p> : keys.length === 0 ? <p className="settings-capability-note">No keys yet.</p> : (
        <ul className="account-keys">
          {keys.map((key) => (
            <li key={key.id} className={key.revoked_at ? 'withdrawn' : undefined}>
              <div><strong>{key.name}</strong> <code>{key.hint}</code></div>
              <div className="account-key-meta">Made {when(key.created_at)} · last used {when(key.last_used_at)}{key.revoked_at ? ` · withdrawn ${when(key.revoked_at)}` : ''}</div>
              {!key.revoked_at && <Button size="sm" variant="danger" onClick={() => void withdraw(key)}>Withdraw</Button>}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
