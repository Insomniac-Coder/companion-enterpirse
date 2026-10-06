import test from 'node:test';
import assert from 'node:assert/strict';
import { apiKeyNameProblem, getMe, isPlatformAdmin, listApiKeys, onSignInNeeded, signInUrl } from '../src/services/account.ts';

const answer = (status, body) => async () => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

test('a browser that is not signed in is told so, not handed an error', async () => {
  globalThis.fetch = answer(401, { error: 'sign in required' });
  assert.equal(await getMe(), null);
  globalThis.fetch = answer(200, { id: 'local', name: 'You', email: '', via: 'local', sign_in: false });
  assert.equal((await getMe()).id, 'local');
  globalThis.fetch = answer(500, { error: 'down' });
  await assert.rejects(getMe(), 'an unreachable server is not "signed out"');
});

test('a session that ends mid-use brings back the sign-in page', async () => {
  let asked = 0;
  const stop = onSignInNeeded(() => { asked += 1; });
  globalThis.fetch = answer(401, { error: 'sign in required', hint: 'Sign in with your company account.' });
  await assert.rejects(listApiKeys(), /sign in required/);
  assert.equal(asked, 1);
  stop();
  await assert.rejects(listApiKeys());
  assert.equal(asked, 1, 'a stopped listener hears nothing');
});

test('a key name is checked here first, by the same rule as the server', () => {
  assert.match(apiKeyNameProblem('   '), /Name the key/);
  assert.match(apiKeyNameProblem('x'.repeat(81)), /80 characters/);
  assert.equal(apiKeyNameProblem('  Build server  '), null);
  assert.equal(apiKeyNameProblem('é'.repeat(80)), null, 'characters, not bytes');
});

test('signing in comes back to the same page on this server', () => {
  assert.equal(signInUrl('/settings?tab=keys'), '/api/auth/login?return_to=%2Fsettings%3Ftab%3Dkeys');
});

test('only a platform admin gets the controls that change the server for everyone', () => {
  const person = (roles) => ({ id: 'x', name: 'X', email: '', via: 'session', sign_in: true, roles, team_admin_of: [], groups: [] });
  assert.equal(isPlatformAdmin(person(['platform_admin'])), true);
  assert.equal(isPlatformAdmin(person([])), false);
  assert.equal(isPlatformAdmin(person(['auditor', 'team_admin'])), false, 'reading people is not running the server');
  assert.equal(isPlatformAdmin({ ...person(['platform_admin', 'auditor']), id: 'local', via: 'local', sign_in: false }), true, "the laptop's one person");
  assert.equal(isPlatformAdmin(undefined), true, 'server unreachable: the app as it always was; the server still refuses');
});
