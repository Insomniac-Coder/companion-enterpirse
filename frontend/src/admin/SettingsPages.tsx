// The dashboard's settings pages (Phase 1 task 9, after the owner's review): the service's own
// settings (model server, web search, rules, privacy and logging), and People's defaults, the
// company's and each group's defaults for the settings people choose in Companion, with a lock
// beside each field. People's own choices are never edited here.
import { useEffect, useState } from 'react';
import ServiceSettings, { LockContext, FieldsContext, SaveBar, useSettingsDraft, type ServiceSection, type SettingsScope } from '../components/SettingsPanel';
import { Dialog } from '../ui/primitives';
import { PERSONAL_SECTIONS, PersonalSection } from '../components/PersonalSettings';
import { changedSettings, modelOwnValues, withValues } from '../components/settingsForm';
import { pushToast, type Toast } from '../components/Toasts';
import {
  groupFields, listGroups, lockSetting, modelSettings, saveCompanySettings, saveGroupSettings, saveModelSettings, settingsOverview, unlockSetting,
  type Group, type SettingsLock, type SettingsOverview,
} from '../services/admin';
import { getSettingFields } from '../services/settingsFields';
import { Button, Lamp } from '../ui/primitives';
import { Icon } from '../ui/Icon';

type Notify = (kind: 'info' | 'success' | 'warning' | 'error', text: string) => void;

function companyScope(): SettingsScope {
  return {
    load: () => settingsOverview().then((next) => next.company),
    save: (changed) => saveCompanySettings(changed),
  };
}

function groupScope(id: string): SettingsScope {
  return {
    load: () => settingsOverview().then((next) => withValues(next.company, next.groups.find((group) => group.group_id === id)?.settings)),
    fields: () => getSettingFields().then(groupFields),
    save: async (_changed, all) => {
      const company = (await settingsOverview()).company;
      const own = changedSettings(company, all);
      await saveGroupSettings(id, own);
      return withValues(company, own);
    },
  };
}

/** One model's own settings: the company's defaults with its own values; saving keeps only what
 *  differs, and only the fields a model has (context, sampling, performance, hardware). */
function modelScope(id: string): SettingsScope {
  return {
    load: () => modelSettings(id).then((found) => found.effective),
    save: async (_changed, all) => {
      const company = (await settingsOverview()).company;
      const own = modelOwnValues(changedSettings(company, all));
      await saveModelSettings(id, own);
      return withValues(company, own);
    },
  };
}

/** A model's Settings, from the Models page. */
export function ModelSettingsDialog({ model, onClose, setToasts }: { model: { id: string; name: string }; onClose: () => void; setToasts: React.Dispatch<React.SetStateAction<Toast[]>> }) {
  return (
    <Dialog size="xl" className="settings-dialog model-settings-dialog" icon="sliders" title={`Settings for ${model.name}`} description="Used from this model's next load." onClose={onClose}>
      <div className="model-settings-body">
        <ServiceSettings key={model.id} sections={['model']} scope={modelScope(model.id)} setToasts={setToasts} model={model} bare />
      </div>
    </Dialog>
  );
}

/** The lock beside a field: locked (with its reason; click to unlock), or a lock to set with a reason. */
function LockControl({ path, groupId, locks, onChange, notify }: { path: string; groupId: string | null; locks: SettingsLock[]; onChange: () => void; notify: Notify }) {
  const [asking, setAsking] = useState(false);
  const [reason, setReason] = useState('');
  const lock = locks.find((item) => item.path === path && (item.group_id ?? null) === groupId);
  const who = groupId ? 'this group' : 'everyone';
  if (lock) {
    return (
      <button type="button" className="lock-ctl locked" title={`Locked for ${who}${lock.reason ? `: ${lock.reason}` : ''}. Click to unlock.`}
        onClick={() => void unlockSetting(path, groupId).then(() => { notify('success', `Unlocked ${path}.`); onChange(); }, (e) => notify('error', e.message))}>
        <Icon name="lock" size={13} />Locked
      </button>
    );
  }
  if (asking) {
    return (
      <span className="lock-ctl asking">
        <input autoFocus aria-label="Why it is locked (people see this)" placeholder="Why (people see this)" value={reason} maxLength={300} onChange={(event) => setReason(event.target.value)}
          onKeyDown={(event) => { if (event.key === 'Escape') setAsking(false); }} />
        <button type="button" className="btn sm" onClick={() => void lockSetting(path, groupId, reason.trim()).then(() => { setAsking(false); setReason(''); notify('success', `Locked ${path} for ${who}.`); onChange(); }, (e) => notify('error', e.message))}>Lock</button>
        <button type="button" className="btn ghost sm" onClick={() => setAsking(false)}>Cancel</button>
      </span>
    );
  }
  return <button type="button" className="lock-ctl" aria-label={`Lock ${path} for ${who}`} title={`Lock for ${who}: nobody below can change it`} onClick={() => setAsking(true)}><Icon name="lock" size={13} /></button>;
}

/** Everyone (the company) or one group, and the group's priority. */
function ScopePicker({ groups, value, onChange, overview, canChange, notify, onSaved }: {
  groups: Group[];
  value: string | null;
  onChange: (next: string | null) => void;
  overview: SettingsOverview;
  canChange: boolean;
  notify: Notify;
  onSaved: () => void;
}) {
  const own = value ? overview.groups.find((group) => group.group_id === value) : undefined;
  const [priority, setPriority] = useState(String(own?.priority ?? 100));
  useEffect(() => { setPriority(String(own?.priority ?? 100)); }, [value, own?.priority]);
  return (
    <div className="admin-group-bar">
      <label>Applies to
        <select value={value ?? ''} onChange={(event) => onChange(event.target.value || null)}>
          <option value="">Everyone (the company)</option>
          {groups.map((group) => <option key={group.id} value={group.id}>{group.name || group.external_id}</option>)}
        </select>
      </label>
      {value && canChange && (
        <>
          <label>Priority when groups disagree (lowest wins)
            <input type="number" min={0} max={10000} value={priority} onChange={(event) => setPriority(event.target.value)} />
          </label>
          <Button size="sm" disabled={!/^\d+$/.test(priority) || Number(priority) === (own?.priority ?? 100)} onClick={() => {
            void saveGroupSettings(value, own?.settings ?? {}, Number(priority)).then(() => { notify('success', 'Priority saved.'); onSaved(); }, (e) => notify('error', e.message));
          }}>Save priority</Button>
        </>
      )}
    </div>
  );
}

function useOverview(notify: Notify) {
  const [overview, setOverview] = useState<SettingsOverview | null>(null);
  const [groups, setGroups] = useState<Group[]>([]);
  const refresh = () => Promise.all([settingsOverview(), listGroups()])
    .then(([next, nextGroups]) => { setOverview(next); setGroups(nextGroups); })
    .catch((e) => notify('error', e.message));
  useEffect(() => { void refresh(); }, []);
  return { overview, groups, refresh };
}


/** One service page. Rules can differ per group and be locked for the company; the rest are the
 *  company's alone. */
/** `signIn`: a company server; a laptop install has one person, so there is nobody for a lock to bind. */
export function ServiceSettingsPage({ sections, canChange, signIn, setToasts, notify }: { sections: ServiceSection[]; canChange: boolean; signIn: boolean; setToasts: React.Dispatch<React.SetStateAction<Toast[]>>; notify: Notify }) {
  const { overview, groups, refresh } = useOverview(notify);
  const [group, setGroup] = useState<string | null>(null);
  if (!overview) return <div className="page"><div className="page-inner"><p className="settings-capability-note"><Lamp state="caution" pulse /> Loading settings…</p></div></div>;
  const perGroup = sections.includes('rules');
  const lock = perGroup && canChange && signIn && !group ? (path: string) => <LockControl path={path} groupId={null} locks={overview.locks} onChange={() => void refresh()} notify={notify} /> : null;
  return (
    <>
      {perGroup && groups.length > 0 && <ScopePicker groups={groups} value={group} onChange={setGroup} overview={overview} canChange={canChange} notify={notify} onSaved={() => void refresh()} />}
      <LockContext.Provider value={lock}>
        <ServiceSettings
          key={`${sections.join('-')}-${group ?? 'company'}`}
          sections={sections}
          scope={group ? groupScope(group) : companyScope()}
          setToasts={setToasts}
          readOnly={!canChange}
          note={[canChange ? '' : 'Read only: an auditor sees these but does not change them.', lock ? 'A lock beside a field keeps groups from setting their own value.' : ''].filter(Boolean).join(' ') || undefined}
        />
      </LockContext.Provider>
    </>
  );
}

/** The company's and each group's defaults for the settings people choose themselves. */
export function PeopleDefaultsPage({ canChange, signIn, setToasts, notify }: { canChange: boolean; signIn: boolean; setToasts: React.Dispatch<React.SetStateAction<Toast[]>>; notify: Notify }) {
  const { overview, groups, refresh } = useOverview(notify);
  const [group, setGroup] = useState<string | null>(null);
  if (!overview) return <div className="page"><div className="page-inner"><p className="settings-capability-note"><Lamp state="caution" pulse /> Loading settings…</p></div></div>;
  return (
    <>
      {groups.length > 0 && <ScopePicker groups={groups} value={group} onChange={setGroup} overview={overview} canChange={canChange} notify={notify} onSaved={() => void refresh()} />}
      <DefaultsEditor key={group ?? 'company'} group={group} locks={overview.locks} canChange={canChange} signIn={signIn} setToasts={setToasts} notify={notify} onLocksChanged={() => void refresh()} />
    </>
  );
}

function DefaultsEditor({ group, locks, canChange, signIn, setToasts, notify, onLocksChanged }: {
  group: string | null;
  locks: SettingsLock[];
  canChange: boolean;
  signIn: boolean;
  setToasts: React.Dispatch<React.SetStateAction<Toast[]>>;
  notify: Notify;
  onLocksChanged: () => void;
}) {
  const draft = useSettingsDraft(group ? groupScope(group) : companyScope(), (message) => pushToast(setToasts, 'error', message));
  const lock = canChange && signIn ? (path: string) => <LockControl path={path} groupId={group} locks={locks} onChange={onLocksChanged} notify={notify} /> : null;
  if (!draft.s) return <div className="page"><div className="page-inner"><p className="settings-capability-note"><Lamp state="caution" pulse /> {draft.error || 'Loading settings…'}</p></div></div>;
  return (
    <FieldsContext.Provider value={draft.fields}>
      <LockContext.Provider value={lock}>
        <div className="page">
          <div className="page-inner">
            <div className="settings-page">
              <p className="settings-capability-note">
                {group
                  ? 'What this group’s members start with instead of the company’s defaults. Only what differs from the company is kept.'
                  : signIn ? 'What everyone starts with. People change these in Companion’s Settings unless a field is locked; a group can set its own.' : 'This install has one person, so these are your own settings, the same ones Companion’s Settings changes. Locks and group values matter on a company server.'}
                {!canChange ? ' Read only: an auditor sees these but does not change them.' : signIn ? ' A lock keeps everyone below from changing the field, and they see the reason.' : ''}
              </p>
              <fieldset className="settings-readonly" disabled={!canChange}>
                {PERSONAL_SECTIONS.map((section) => <PersonalSection key={section.id} id={section.id} s={draft.s} set={draft.set} />)}
              </fieldset>
              {draft.dirty && canChange && <SaveBar saving={draft.saving} onSave={() => void draft.save().then((saved) => { if (saved) notify('success', 'Defaults saved. People get them with their next request.'); })} onDiscard={draft.discard} />}
            </div>
          </div>
        </div>
      </LockContext.Provider>
    </FieldsContext.Provider>
  );
}
