import { useEffect, useMemo, useState } from 'react';
import SettingsPanel, { type SettingsScope } from '../components/SettingsPanel';
import { changedSettings, withValues } from '../components/settingsForm';
import type { Toast } from '../components/Toasts';
import {
  groupFields, listGroups, lockSetting, serverTime, saveCompanySettings, saveGroupSettings, settingsOverview, unlockSetting,
  type Group, type SettingsOverview,
} from '../services/admin';
import { getSettingFields, type SettingFields } from '../services/settingsFields';
import { Button, Section } from '../ui/primitives';

type Notify = (kind: 'info' | 'success' | 'warning' | 'error', text: string) => void;
type View = { kind: 'company' } | { kind: 'group'; id: string } | { kind: 'locks' };

const KIND_NOTE = {
  personal: 'people may choose their own',
  policy: 'the company or a group decides',
  machine: 'the company only',
} as const;

export default function CompanySettingsPage({ canChange, setToasts, notify }: { canChange: boolean; setToasts: React.Dispatch<React.SetStateAction<Toast[]>>; notify: Notify }) {
  const [overview, setOverview] = useState<SettingsOverview | null>(null);
  const [groups, setGroups] = useState<Group[]>([]);
  const [fields, setFields] = useState<SettingFields>({});
  const [view, setView] = useState<View>({ kind: 'company' });
  const [error, setError] = useState('');
  const [lock, setLock] = useState({ path: '', group: '', reason: '' });
  const [priority, setPriority] = useState('');

  const refresh = () => Promise.all([settingsOverview(), listGroups(), getSettingFields()])
    .then(([next, nextGroups, nextFields]) => { setOverview(next); setGroups(nextGroups); setFields(nextFields); setError(''); })
    .catch((e) => setError(e.message));
  useEffect(() => { void refresh(); }, []);

  const groupValues = (id: string) => overview?.groups.find((group) => group.group_id === id);
  const groupName = (id: string | null) => {
    if (!id) return 'Everyone';
    const group = groups.find((item) => item.id === id);
    return group?.name || group?.external_id || id;
  };
  useEffect(() => {
    if (view.kind === 'group') setPriority(String(groupValues(view.id)?.priority ?? 100));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view, overview]);

  const lockable = useMemo(() => Object.entries(fields).filter(([, field]) => field.kind !== 'machine').map(([path]) => path).sort(), [fields]);

  if (error) return <div className="page"><div className="page-inner"><p className="settings-capability-note">{error}</p></div></div>;
  if (!overview) return <div className="page"><div className="page-inner"><p className="settings-capability-note">Loading settings…</p></div></div>;

  const companyScope: SettingsScope = {
    load: () => settingsOverview().then((next) => next.company),
    save: async (changed) => {
      const saved = await saveCompanySettings(changed);
      void refresh();
      return saved;
    },
    note: canChange
      ? 'The company’s values: everyone starts from these. A group can set personal and policy fields for its members, and people choose their own personal fields unless a field is locked. Machine fields (the model server, hardware) are the company’s alone.'
      : 'The company’s values, read only: an auditor sees them but does not change them.',
  };

  const groupScope = (id: string): SettingsScope => ({
    load: () => settingsOverview().then((next) => withValues(next.company, next.groups.find((group) => group.group_id === id)?.settings)),
    fields: () => getSettingFields().then(groupFields),
    save: async (_changed, all) => {
      const company = (await settingsOverview()).company;
      const own = changedSettings(company, all);
      await saveGroupSettings(id, own);
      void refresh();
      return withValues(company, own);
    },
    note: `What ${groupName(id)} gets instead of the company’s values. Only what differs from the company is kept, so a field set back to the company’s value follows the company again. When a person is in several groups, the lowest priority number wins.`,
  });

  const tabs: { key: string; label: string; view: View }[] = [
    { key: 'company', label: 'Company', view: { kind: 'company' } },
    ...groups.map((group) => ({ key: `group-${group.id}`, label: group.name || group.external_id, view: { kind: 'group', id: group.id } as View })),
    { key: 'locks', label: `Locks (${overview.locks.length})`, view: { kind: 'locks' } },
  ];
  const active = view.kind === 'group' ? `group-${view.id}` : view.kind;

  return (
    <>
      <div className="admin-tabs" role="tablist" aria-label="Whose settings">
        {tabs.map((tab) => (
          <button type="button" role="tab" key={tab.key} aria-selected={active === tab.key} onClick={() => setView(tab.view)}>{tab.label}</button>
        ))}
      </div>
      {view.kind === 'company' && (canChange
        ? <SettingsPanel key="company" setToasts={setToasts} scope={companyScope} />
        : <div className="page"><div className="page-inner admin-page"><p className="settings-capability-note">{companyScope.note}</p><pre className="admin-json">{JSON.stringify(overview.company, null, 2)}</pre></div></div>)}
      {view.kind === 'group' && (
        <>
          {canChange && (
            <div className="admin-group-bar">
              <label>Priority when groups disagree (lowest wins)
                <input type="number" min={0} max={10000} value={priority} onChange={(event) => setPriority(event.target.value)} />
              </label>
              <Button size="sm" disabled={!/^\d+$/.test(priority) || Number(priority) === (groupValues(view.id)?.priority ?? 100)} onClick={() => {
                void saveGroupSettings(view.id, groupValues(view.id)?.settings ?? {}, Number(priority))
                  .then(() => { notify('success', 'Priority saved.'); return refresh(); })
                  .catch((e) => notify('error', e.message));
              }}>Save priority</Button>
            </div>
          )}
          {canChange
            ? <SettingsPanel key={view.id} setToasts={setToasts} scope={groupScope(view.id)} />
            : <div className="page"><div className="page-inner admin-page"><pre className="admin-json">{JSON.stringify(groupValues(view.id)?.settings ?? {}, null, 2)}</pre></div></div>}
        </>
      )}
      {view.kind === 'locks' && (
        <div className="page">
          <div className="page-inner admin-page">
            <p className="settings-capability-note">A locked field shows as locked on the settings screen, with the reason, and nobody below can change it: a company lock binds everyone, a group lock that group's members.</p>
            {canChange && (
              <Section title="Lock a field" icon="lock">
                <div className="admin-inline-form">
                  <select aria-label="Field" value={lock.path} onChange={(event) => setLock({ ...lock, path: event.target.value })}>
                    <option value="">Choose a field</option>
                    {lockable.map((path) => <option key={path} value={path}>{path} ({KIND_NOTE[fields[path].kind]})</option>)}
                  </select>
                  <select aria-label="For" value={lock.group} onChange={(event) => setLock({ ...lock, group: event.target.value })}>
                    <option value="">Everyone</option>
                    {groups.map((group) => <option key={group.id} value={group.id}>{group.name || group.external_id}</option>)}
                  </select>
                  <input aria-label="Reason" placeholder="Why (people see this)" value={lock.reason} onChange={(event) => setLock({ ...lock, reason: event.target.value })} />
                  <Button size="sm" variant="primary" icon="lock" disabled={!lock.path} onClick={() => {
                    void lockSetting(lock.path, lock.group || null, lock.reason.trim())
                      .then(() => { notify('success', `Locked ${lock.path}.`); setLock({ path: '', group: '', reason: '' }); return refresh(); })
                      .catch((e) => notify('error', e.message));
                  }}>Lock</Button>
                </div>
              </Section>
            )}
            <Section title="Locked fields" icon="list" meta={overview.locks.length}>
              {overview.locks.length === 0 ? <p className="settings-capability-note">Nothing is locked: people and groups may change every field their level allows.</p> : (
                <table className="admin-table">
                  <thead><tr><th>Field</th><th>For</th><th>Reason</th><th>Since</th>{canChange && <th />}</tr></thead>
                  <tbody>
                    {overview.locks.map((item) => (
                      <tr key={`${item.path}-${item.group_id ?? ''}`}>
                        <td className="admin-mono">{item.path}</td>
                        <td>{groupName(item.group_id)}</td>
                        <td>{item.reason || <span className="admin-faint">No reason given</span>}</td>
                        <td className="admin-mono">{serverTime(item.set_at, true)}</td>
                        {canChange && <td className="admin-actions"><Button size="sm" variant="ghost" onClick={() => {
                          void unlockSetting(item.path, item.group_id)
                            .then(() => { notify('success', `Unlocked ${item.path}.`); return refresh(); })
                            .catch((e) => notify('error', e.message));
                        }}>Unlock</Button></td>}
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </Section>
          </div>
        </div>
      )}
    </>
  );
}
