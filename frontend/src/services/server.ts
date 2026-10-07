// Which server the apps talk to, and how a request proves who is asking (Phase 1, task 9). In a
// browser that is the page's own server and its sign-in cookie; the desktop app (Phase 3) points it
// at a saved server and passes that server's token. Every request goes through `serverFetch`.

let base = '';
let token = '';

/** Talk to the server at `address` (empty: this page's own), sending `signInToken` when given. */
export function useServer(address: string, signInToken = ''): void {
  base = address.trim().replace(/\/+$/, '');
  token = signInToken;
}

/** `path` on the server, for links and image addresses. */
export function serverUrl(path: string): string {
  return base + path;
}

/** `fetch` against the server, carrying the token when there is one. */
export function serverFetch(path: string, init: RequestInit = {}): Promise<Response> {
  if (!token) return fetch(base + path, init);
  const headers = new Headers(init.headers);
  headers.set('authorization', `Bearer ${token}`);
  return fetch(base + path, { ...init, headers });
}
