// What the dashboard reads and changes (Phase 1, task 9): people and their roles, groups, the
// company's and groups' settings with their locks, and the audit records. The routes are the
// server's admin routes; a platform admin changes, an auditor reads.
import { serverFetch } from './server.ts';
import { signInNeeded, type Me } from './account.ts';
import type { SettingFields } from './settingsFields.ts';

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await serverFetch(path, init);
  if (response.status === 401) signInNeeded();
  if (!response.ok) {
    let message = `request failed: ${response.status}`;
    try {
      const body = await response.json();
      if (body.error) message = `${body.error} — ${body.hint ?? ''}`.trim();
    } catch { /* not JSON */ }
    throw new Error(message);
  }
  return response.json();
}

const send = (method: string, body?: unknown): RequestInit => ({
  method,
  headers: { 'content-type': 'application/json' },
  body: body === undefined ? undefined : JSON.stringify(body),
});

export interface RoleGrant {
  role: string;
  group_id: string | null;
  group_name: string;
  /** "directory" (the company's sign-in), "config" (the server's admin list) or "admin" (given here). */
  source: string;
  granted_by: string;
  granted_at: string;
}

export interface Person {
  id: string;
  name: string;
  email: string;
  last_seen_at: string;
  roles: RoleGrant[];
  groups: { id: string; name: string; external_id: string }[];
}

export interface Group {
  id: string;
  external_id: string;
  name: string;
  members: number;
}

export const listPeople = () => call<Person[]>('/api/admin/users');
export const listGroups = () => call<Group[]>('/api/admin/groups');
export const nameGroup = (id: string, name: string) => call(`/api/admin/groups/${encodeURIComponent(id)}`, send('PATCH', { name }));
export const grantRole = (user: string, role: string, groupId?: string) =>
  call(`/api/admin/users/${encodeURIComponent(user)}/roles`, send('POST', { role, group_id: groupId ?? null }));
export const withdrawRole = (user: string, role: string, groupId?: string | null) =>
  call(`/api/admin/users/${encodeURIComponent(user)}/roles`, send('DELETE', { role, group_id: groupId ?? null }));

export interface SettingsLock {
  path: string;
  group_id: string | null;
  reason: string;
  set_by: string;
  set_at: string;
}

export interface SettingsOverview {
  company: any;
  groups: { group_id: string; name: string; external_id: string; settings: any; priority: number }[];
  locks: SettingsLock[];
}

export const settingsOverview = () => call<SettingsOverview>('/api/admin/settings');
export const saveCompanySettings = (changed: Record<string, unknown>) => call<any>('/api/admin/settings/company', send('PUT', { settings: changed }));
export const saveGroupSettings = (group: string, settings: Record<string, unknown>, priority?: number) =>
  call(`/api/admin/settings/groups/${encodeURIComponent(group)}`, send('PUT', { settings, priority }));
export const lockSetting = (path: string, groupId: string | null, reason: string) => call('/api/admin/settings/locks', send('PUT', { path, group_id: groupId, reason }));
export const unlockSetting = (path: string, groupId: string | null) => call('/api/admin/settings/locks', send('DELETE', { path, group_id: groupId }));

/** A model's own settings (only what differs from the company's), and what loading it uses. */
export const modelSettings = (id: string) => call<{ model_id: string; settings: any; effective: any }>(`/api/admin/models/${encodeURIComponent(id)}/settings`);
export const saveModelSettings = (id: string, settings: Record<string, unknown>) => call(`/api/admin/models/${encodeURIComponent(id)}/settings`, send('PUT', { settings }));

export interface Server {
  id: string;
  name: string;
  os: string;
  cpu: string;
  ram_gb: number;
  gpus: { model: string; vram_gb: number }[];
  /** The models it hosts now. */
  models: { id: string; name: string }[];
  state: 'serving' | 'idle';
}

/** The servers that host models (one today). */
export const listServers = () => call<{ servers: Server[] }>('/api/admin/servers');

export interface AuditRecord {
  seq: number;
  at: string;
  user_id: string;
  via: string;
  address: string;
  device: string;
  action: string;
  target: string;
  model: string;
  prompt_tokens: number;
  generated_tokens: number;
  allowed_by: string;
  outcome: string;
  detail: Record<string, unknown>;
}

/** The newest records first; `before` continues from an earlier page. */
export const auditRecords = (before?: number, limit = 100) =>
  call<{ records: AuditRecord[] }>(`/api/admin/audit?limit=${limit}${before ? `&before=${before}` : ''}`);

export const ACTION_LABELS: Record<string, string> = {
  model_request: 'Model request',
  tool_call: 'Tool call',
  web_search: 'Web search',
  approval: 'Approval',
  admin_change: 'Admin change',
  settings_change: 'Settings change',
  sign_in: 'Sign-in',
  sign_out: 'Sign-out',
  api_key: 'API key',
};

/** What a record was about, in one line: the tool and its arguments, the query, the route. */
export function auditWhat(record: AuditRecord): string {
  const detail = record.detail ?? {};
  const text = (value: unknown) => (typeof value === 'string' ? value : value == null ? '' : JSON.stringify(value));
  switch (record.action) {
    case 'tool_call':
      return `${text(detail.tool)} ${text(detail.args)}`.trim();
    case 'web_search':
      return `${text(detail.query)}${detail.provider ? ` (${text(detail.provider)})` : ''}`;
    case 'approval':
      return `${text(detail.tool)} in ${record.target}`;
    case 'model_request':
      return `${text(detail.kind)} in ${record.target || 'no conversation'}`;
    case 'sign_in':
      return detail.reason ? text(detail.reason) : '';
    default:
      return `${record.target}${detail.body ? ` ${text(detail.body)}` : ''}`;
  }
}

/** The person a record names: a name from the people list, "the server" for its own work. */
export function auditWho(record: AuditRecord, people: Map<string, Person>): string {
  if (record.user_id === 'system') return 'The server';
  if (record.user_id === 'local') return 'You (this PC)';
  if (!record.user_id) return 'Unknown';
  return people.get(record.user_id)?.name || record.user_id;
}

export const ROLE_LABEL: Record<string, string> = { platform_admin: 'Platform admin', auditor: 'Auditor', team_admin: 'Team admin' };
export const SOURCE_NOTE: Record<string, string> = {
  directory: 'From the company directory: change it there',
  config: 'From the server’s admin list (COMPANION_PLATFORM_ADMINS)',
  admin: 'Given here',
};

/** Why `grant` cannot be withdrawn here by `me`, or null when it can (the server checks the same). */
export function cannotWithdraw(grant: RoleGrant, person: Person, me?: Me): string | null {
  if (grant.source !== 'admin') return SOURCE_NOTE[grant.source] ?? 'Not given here';
  if (grant.role === 'platform_admin' && person.id === me?.id) return 'Another platform admin can withdraw your own';
  return null;
}

/** Fields a group may set: personal and policy ones; the machine's are the company's alone. */
export function groupFields(fields: SettingFields): SettingFields {
  return Object.fromEntries(Object.entries(fields).map(([path, field]) => [
    path,
    { ...field, locked_by: null, reason: '', editable: field.kind !== 'machine' },
  ]));
}

/** A time as PostgreSQL writes it ("2026-10-07 02:10:11.5+00"), for people; empty: never. */
export function serverTime(value: string, dateOnly = false): string {
  if (!value) return 'Never';
  const at = new Date(value.replace(' ', 'T').replace(/([+-]\d\d)$/, '$1:00'));
  if (Number.isNaN(at.getTime())) return value;
  return dateOnly ? at.toLocaleDateString() : at.toLocaleString();
}

/** Roles a platform admin hands out here; team admin needs a group. */
export const GRANTABLE_ROLES = [
  { role: 'platform_admin', label: 'Platform admin' },
  { role: 'auditor', label: 'Auditor' },
  { role: 'team_admin', label: 'Team admin' },
] as const;

/** The dashboard's pages, and who sees each: a platform admin all, an auditor the reading ones. */
export const DASHBOARD_PAGES = [
  { id: 'people', label: 'People and groups', icon: 'users', auditor: true, group: 'People' },
  { id: 'defaults', label: 'People’s defaults', icon: 'sliders', auditor: true, group: 'People' },
  { id: 'rules', label: 'Rules', icon: 'shield', auditor: true, group: 'People' },
  { id: 'models', label: 'Models', icon: 'layers', auditor: false, group: 'Models' },
  { id: 'model-server', label: 'Model server', icon: 'sliders', auditor: false, group: 'Models' },
  { id: 'system', label: 'Runtime and diagnostics', icon: 'gauge', auditor: false, group: 'Models' },
  { id: 'servers', label: 'Servers', icon: 'cpu', auditor: false, group: 'Models' },
  { id: 'search', label: 'Web search', icon: 'globe', auditor: true, group: 'Service' },
  { id: 'privacy', label: 'Privacy and logging', icon: 'lock', auditor: true, group: 'Service' },
  { id: 'audit', label: 'Audit records', icon: 'list', auditor: true, group: 'Service' },
] as const;

export type DashboardPage = (typeof DASHBOARD_PAGES)[number]['id'];

/** The page a dashboard link names (`/admin/#models`), or the first this person may see. */
export function pageFromHash(hash: string, platformAdmin: boolean): DashboardPage {
  const wanted = hash.replace(/^#/, '');
  const page = DASHBOARD_PAGES.find(({ id }) => id === wanted);
  if (page && (platformAdmin || page.auditor)) return page.id;
  return DASHBOARD_PAGES[0].id;
}
