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
