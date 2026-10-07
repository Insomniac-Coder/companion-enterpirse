import { useEffect, useState } from 'react';
import { cannotWithdraw, GRANTABLE_ROLES, serverTime, grantRole, listGroups, listPeople, nameGroup, ROLE_LABEL, SOURCE_NOTE, withdrawRole, type Group, type Person } from '../services/admin';
import type { Me } from '../services/account';
import { Button, Section } from '../ui/primitives';
import { Icon } from '../ui/Icon';

type Notify = (kind: 'info' | 'success' | 'warning' | 'error', text: string) => void;

export default function PeoplePage({ canChange, me, notify }: { canChange: boolean; me?: Me; notify: Notify }) {
  const [people, setPeople] = useState<Person[] | null>(null);
  const [groups, setGroups] = useState<Group[]>([]);
  const [error, setError] = useState('');
  const [giving, setGiving] = useState<{ person: string; role: string; group: string } | null>(null);
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null);

  const refresh = () => Promise.all([listPeople(), listGroups()])
    .then(([nextPeople, nextGroups]) => { setPeople(nextPeople); setGroups(nextGroups); setError(''); })
    .catch((e) => setError(e.message));
  useEffect(() => { void refresh(); }, []);

  const act = (work: Promise<unknown>, done: string) => work.then(() => { notify('success', done); return refresh(); }).catch((e) => notify('error', e.message));

  if (error) return <div className="page"><div className="page-inner"><p className="settings-capability-note">{error}</p></div></div>;
  if (!people) return <div className="page"><div className="page-inner"><p className="settings-capability-note">Loading people…</p></div></div>;

  const roleNeedsGroup = giving?.role === 'team_admin';
  const giveReady = !!giving && (!roleNeedsGroup || !!giving.group);

  return (
    <div className="page">
      <div className="page-inner admin-page">
        {!me?.sign_in && (
          <p className="settings-capability-note">This install has no sign-in: its one person (you) has every role. People appear here once the server signs in with the company account.</p>
        )}
        <Section title="People" icon="users" meta={people.length}>
          {people.length === 0 ? <p className="settings-capability-note">Nobody has signed in yet. People appear here after their first sign-in.</p> : (
            <table className="admin-table">
              <thead><tr><th>Person</th><th>Groups</th><th>Roles</th><th>Last seen</th>{canChange && <th />}</tr></thead>
              <tbody>
                {people.map((person) => (
                  <tr key={person.id}>
                    <td><strong>{person.name || person.email}</strong><small>{person.email}</small></td>
                    <td>{person.groups.length ? person.groups.map((group) => <span key={group.id} className="admin-chip">{group.name || group.external_id}</span>) : <span className="admin-faint">None</span>}</td>
                    <td>
                      {person.roles.length === 0 && <span className="admin-faint">User</span>}
                      {person.roles.map((grant) => {
                        const blocked = cannotWithdraw(grant, person, me);
                        return (
                          <span key={`${grant.role}-${grant.group_id ?? ''}-${grant.source}`} className="admin-chip" title={SOURCE_NOTE[grant.source]}>
                            {ROLE_LABEL[grant.role] ?? grant.role}{grant.group_name ? ` · ${grant.group_name}` : ''}
                            {canChange && !blocked && (
                              <button type="button" aria-label={`Withdraw ${ROLE_LABEL[grant.role] ?? grant.role} from ${person.name}`} onClick={() => void act(withdrawRole(person.id, grant.role, grant.group_id), `Withdrew ${ROLE_LABEL[grant.role]} from ${person.name}.`)}>
                                <Icon name="x" size={11} />
                              </button>
                            )}
                          </span>
                        );
                      })}
                    </td>
                    <td className="admin-mono">{serverTime(person.last_seen_at)}</td>
                    {canChange && (
                      <td className="admin-actions">
                        {giving?.person === person.id ? (
                          <span className="admin-inline-form">
                            <select aria-label="Role" value={giving.role} onChange={(event) => setGiving({ ...giving, role: event.target.value, group: '' })}>
                              {GRANTABLE_ROLES.map(({ role, label }) => <option key={role} value={role}>{label}</option>)}
                            </select>
                            {roleNeedsGroup && (
                              <select aria-label="Group" value={giving.group} onChange={(event) => setGiving({ ...giving, group: event.target.value })}>
                                <option value="">Choose a group</option>
                                {groups.map((group) => <option key={group.id} value={group.id}>{group.name || group.external_id}</option>)}
                              </select>
                            )}
                            <Button size="sm" variant="primary" disabled={!giveReady} title={giveReady ? undefined : 'A team admin looks after one group: choose it'} onClick={() => { const g = giving; setGiving(null); void act(grantRole(person.id, g.role, g.role === 'team_admin' ? g.group : undefined), `Gave ${ROLE_LABEL[g.role]} to ${person.name}.`); }}>Give</Button>
                            <Button size="sm" variant="ghost" onClick={() => setGiving(null)}>Cancel</Button>
                          </span>
                        ) : (
                          <Button size="sm" variant="ghost" icon="plus" onClick={() => setGiving({ person: person.id, role: 'auditor', group: '' })}>Role</Button>
                        )}
                      </td>
                    )}
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Section>

        <Section title="Groups" icon="layers" meta={groups.length}>
          {groups.length === 0 ? <p className="settings-capability-note">No groups yet. They come from the company directory: add the groups claim to the sign-in (see the README), and each group appears at its members' next sign-in.</p> : (
            <table className="admin-table">
              <thead><tr><th>Group</th><th>Directory id</th><th>Members</th>{canChange && <th />}</tr></thead>
              <tbody>
                {groups.map((group) => (
                  <tr key={group.id}>
                    <td>
                      {renaming?.id === group.id ? (
                        <input
                          autoFocus
                          aria-label="Group name"
                          value={renaming.name}
                          onChange={(event) => setRenaming({ id: group.id, name: event.target.value })}
                          onKeyDown={(event) => {
                            if (event.key === 'Escape') setRenaming(null);
                            if (event.key === 'Enter' && renaming.name.trim()) { setRenaming(null); void act(nameGroup(group.id, renaming.name.trim()), 'Group renamed.'); }
                          }}
                        />
                      ) : <strong>{group.name || <span className="admin-faint">No name yet</span>}</strong>}
                    </td>
                    <td className="admin-mono">{group.external_id}</td>
                    <td className="admin-mono">{group.members}</td>
                    {canChange && <td className="admin-actions"><Button size="sm" variant="ghost" icon="pencil" onClick={() => setRenaming({ id: group.id, name: group.name })}>Name</Button></td>}
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Section>
      </div>
    </div>
  );
}
