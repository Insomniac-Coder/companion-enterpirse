// Who is signed in, signing in and out, and API keys (Phase 1, task 3). An install without
// sign-in (a laptop) answers /api/me with the local person and never asks anyone to sign in.

export interface Me {
  id: string;
  name: string;
  email: string;
  /** How this request was identified. */
  via: 'local' | 'session' | 'api key';
  /** Whether this server has sign-in at all. */
  sign_in: boolean;
}

export interface ApiKey {
  id: string;
  name: string;
  /** The key's first characters, to tell keys apart. */
  hint: string;
  created_at: string;
  last_used_at: string | null;
  revoked_at: string | null;
}

const signInListeners = new Set<() => void>();

/** Run `listener` whenever the server says this browser is not signed in (a session that ended). */
export function onSignInNeeded(listener: () => void): () => void {
  signInListeners.add(listener);
  return () => { signInListeners.delete(listener); };
}

/** Called by the request helpers on a 401 answer. */
export function signInNeeded() {
  for (const listener of [...signInListeners]) listener();
}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(path, init);
  if (r.status === 401) signInNeeded();
  if (!r.ok) {
    let message = `request failed: ${r.status}`;
    try {
      const body = await r.json();
      if (body.error) message = `${body.error} — ${body.hint ?? ''}`.trim();
    } catch { /* not JSON */ }
    throw new Error(message);
  }
  return r.json();
}

/** The person this browser is signed in as, or null when sign-in is needed. Throws when the server cannot be reached. */
export async function getMe(): Promise<Me | null> {
  const r = await fetch('/api/me');
  if (r.status === 401) return null;
  if (!r.ok) throw new Error(`request failed: ${r.status}`);
  return r.json();
}

/** Where the browser goes to sign in, coming back to `returnTo` on this server. */
export function signInUrl(returnTo: string): string {
  return `/api/auth/login?return_to=${encodeURIComponent(returnTo)}`;
}

export async function signOut(): Promise<void> {
  await call('/api/auth/logout', { method: 'POST' });
}

/** Why `name` cannot name a key, or null when it can. The server checks the same rule. */
export function apiKeyNameProblem(name: string): string | null {
  const trimmed = name.trim();
  if (!trimmed) return 'Name the key after the software that will use it.';
  if ([...trimmed].length > 80) return 'Keep the name to 80 characters.';
  return null;
}

export async function listApiKeys(): Promise<ApiKey[]> {
  return call('/api/me/api-keys');
}

/** A new key; `key` is shown this once. */
export async function createApiKey(name: string): Promise<ApiKey & { key: string }> {
  return call('/api/me/api-keys', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ name: name.trim() }),
  });
}

export async function revokeApiKey(id: string): Promise<void> {
  await call(`/api/me/api-keys/${encodeURIComponent(id)}`, { method: 'DELETE' });
}
