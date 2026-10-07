import test from 'node:test';
import assert from 'node:assert/strict';
import { auditWhat, auditWho, cannotWithdraw, groupFields, pageFromHash, serverTime } from '../src/services/admin.ts';

test('a dashboard link opens its page, and an auditor only the pages for reading', () => {
  assert.equal(pageFromHash('#models', true), 'models');
  assert.equal(pageFromHash('#models', false), 'people', 'an auditor does not manage models');
  assert.equal(pageFromHash('#audit', false), 'audit');
  assert.equal(pageFromHash('', true), 'people');
  assert.equal(pageFromHash('#nonsense', true), 'people');
});

test('only a role given here can be withdrawn here, and never your own platform admin role', () => {
  const ada = { id: 'ada', name: 'Ada', email: '', last_seen_at: '', roles: [], groups: [] };
  const grant = (role, source) => ({ role, source, group_id: null, group_name: '', granted_by: '', granted_at: '' });
  assert.equal(cannotWithdraw(grant('auditor', 'admin'), ada, { id: 'boss' }), null);
  assert.match(cannotWithdraw(grant('auditor', 'directory'), ada, { id: 'boss' }), /directory/);
  assert.match(cannotWithdraw(grant('platform_admin', 'config'), ada, { id: 'boss' }), /COMPANION_PLATFORM_ADMINS/);
  assert.match(cannotWithdraw(grant('platform_admin', 'admin'), ada, { id: 'ada' }), /Another platform admin/);
  assert.equal(cannotWithdraw(grant('auditor', 'admin'), ada, { id: 'ada' }), null, 'your own other roles you may drop');
});

test('a group sets personal and policy fields, never the machine\'s, and ignores the viewer\'s own locks', () => {
  const fields = groupFields({
    'appearance.theme': { kind: 'personal', locked_by: 'Finance', reason: 'house style', editable: false },
    'files.allowed_dirs': { kind: 'policy', locked_by: null, reason: '', editable: false },
    'inference.context_size': { kind: 'machine', locked_by: null, reason: '', editable: false },
  });
  assert.equal(fields['appearance.theme'].editable, true);
  assert.equal(fields['appearance.theme'].locked_by, null);
  assert.equal(fields['files.allowed_dirs'].editable, true);
  assert.equal(fields['inference.context_size'].editable, false);
});

test('each audit record reads as one line: who, and what it was about', () => {
  const people = new Map([['u1', { id: 'u1', name: 'Ada Lovelace' }]]);
  const base = { seq: 1, at: '', via: 'session', address: '', device: '', target: 'c1', model: '', prompt_tokens: 0, generated_tokens: 0, allowed_by: '', outcome: '' };
  assert.equal(auditWho({ ...base, user_id: 'u1' }, people), 'Ada Lovelace');
  assert.equal(auditWho({ ...base, user_id: 'system' }, people), 'The server');
  assert.equal(auditWho({ ...base, user_id: '' }, people), 'Unknown', 'a refused sign-in names nobody');
  assert.equal(auditWhat({ ...base, user_id: 'u1', action: 'tool_call', detail: { tool: 'write_file', args: '{"path":"n.txt"}' } }), 'write_file {"path":"n.txt"}');
  assert.equal(auditWhat({ ...base, user_id: 'u1', action: 'web_search', detail: { query: 'rust axum', provider: 'duckduckgo' } }), 'rust axum (duckduckgo)');
  assert.equal(auditWhat({ ...base, user_id: 'u1', action: 'admin_change', target: 'PUT /api/admin/settings/company', detail: { body: '{}' } }), 'PUT /api/admin/settings/company {}');
});

test('server times read as local times, and an empty one as never', () => {
  const shown = serverTime('2026-10-07 02:10:11.5+00');
  assert.notEqual(shown, 'Invalid Date');
  assert.equal(shown, new Date('2026-10-07T02:10:11.5Z').toLocaleString());
  assert.equal(serverTime(''), 'Never');
});
