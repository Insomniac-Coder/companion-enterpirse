import test from 'node:test';
import assert from 'node:assert/strict';
import { serverFetch, serverUrl, useServer } from '../src/services/server.ts';

test('requests go to the chosen server and carry its token', async () => {
  const sent = [];
  globalThis.fetch = async (url, init) => { sent.push({ url, init }); return new Response('{}'); };

  useServer('');
  await serverFetch('/api/me');
  assert.equal(sent[0].url, '/api/me', 'a browser talks to its own page server');
  assert.equal(sent[0].init.headers, undefined, 'and signs in with its cookie, not a header');

  useServer('https://companion.example.com/', 'cmp_abc');
  await serverFetch('/api/chat', { method: 'POST', headers: { 'content-type': 'application/json' } });
  assert.equal(sent[1].url, 'https://companion.example.com/api/chat');
  assert.equal(sent[1].init.headers.get('authorization'), 'Bearer cmp_abc');
  assert.equal(sent[1].init.headers.get('content-type'), 'application/json', 'the request keeps its own headers');
  assert.equal(serverUrl('/api/x'), 'https://companion.example.com/api/x');
  useServer('');
});

test('the user app names the dashboard but never links to it: it is a separate tool for admins', async () => {
  const { readFileSync, readdirSync } = await import('node:fs');
  const files = ['App.tsx', ...readdirSync(new URL('../src/components/', import.meta.url)).map((name) => `components/${name}`)];
  for (const file of files) {
    const source = readFileSync(new URL(`../src/${file}`, import.meta.url), 'utf8');
    assert.doesNotMatch(source, /serverUrl\(\s*['`"]\/admin|href=\{?['"`]\/admin|location\.href\s*=\s*['"`][^'"`]*\/admin/, `${file} links to the dashboard`);
  }
  const { dashboardAddress } = await import('../src/services/server.ts');
  globalThis.location = { href: 'https://companion.example.com/chat' };
  assert.equal(dashboardAddress(), 'https://companion.example.com/admin/');
  delete globalThis.location;
});
