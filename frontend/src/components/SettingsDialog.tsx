// Companion's Settings: a dialog, as in ChatGPT and Claude, opened from the settings button at the
// bottom of the sidebar (or the command palette). A person's own choices, the tools and plugins
// they can use, and their account. Running the service is the dashboard's, never shown here.
import { useState } from 'react';
import AccountSection from './AccountSection';
import ToolsPage from './ToolsPage';
import { PERSONAL_SECTIONS, PersonalSection, type PersonalSectionId } from './PersonalSettings';
import { FieldsContext, SaveBar, useSettingsDraft } from './SettingsPanel';
import { pushToast, type Toast } from './Toasts';
import type { Me } from '../services/account';
import type { ToolDescriptor } from '../services/api';
import { Dialog, Lamp } from '../ui/primitives';
import { Icon, type IconName } from '../ui/Icon';

export type SettingsSectionId = PersonalSectionId | 'tools' | 'account';

const OTHER_SECTIONS: { id: SettingsSectionId; title: string; icon: IconName }[] = [
  { id: 'tools', title: 'Tools and plugins', icon: 'wrench' },
  { id: 'account', title: 'Account', icon: 'lock' },
];

/** After a save: the open screens follow the new choices at once. */
function applyEverywhere(saved: any) {
  window.dispatchEvent(new CustomEvent('companion:settings', { detail: saved }));
  document.documentElement.dataset.density = saved.appearance?.density ?? 'comfortable';
  document.documentElement.classList.toggle('reduce-motion', !!saved.appearance?.reduce_motion);
  if (saved.appearance?.theme) {
    localStorage.setItem('companion.theme', saved.appearance.theme);
    window.dispatchEvent(new CustomEvent('companion:appearance', { detail: saved.appearance }));
  }
}

export default function SettingsDialog({ section, onSection, onClose, me, setToasts, registry, wsId, onRefreshTools }: {
  section: SettingsSectionId;
  onSection: (next: SettingsSectionId) => void;
  onClose: () => void;
  me?: Me;
  setToasts: React.Dispatch<React.SetStateAction<Toast[]>>;
  registry: ToolDescriptor[];
  wsId: string;
  onRefreshTools: () => void;
}) {
  const notify = (kind: Toast['kind'], text: string) => pushToast(setToasts, kind, text);
  const draft = useSettingsDraft(undefined, (message) => notify('error', message));
  const [closing, setClosing] = useState(false);
  const close = () => {
    if (draft.dirty && !closing) { setClosing(true); return; }
    onClose();
  };
  const save = async () => {
    const saved = await draft.save();
    if (!saved) return;
    applyEverywhere(saved);
    notify('success', 'Settings saved.');
  };
  const nav = [...PERSONAL_SECTIONS.map(({ id, title, icon }) => ({ id: id as SettingsSectionId, title, icon })), ...OTHER_SECTIONS];
  const personal = PERSONAL_SECTIONS.some(({ id }) => id === section);

  return (
    <Dialog
      size="xl"
      className="settings-dialog"
      title="Settings"
      icon="settings"
      onClose={close}
      footer={closing ? (
        <div className="settings-dialog-leave" role="alert">
          <span>You have unsaved changes.</span>
          <button type="button" className="btn ghost sm" onClick={() => { draft.discard(); onClose(); }}>Discard and close</button>
          <button type="button" className="btn sm" onClick={() => setClosing(false)}>Keep editing</button>
        </div>
      ) : draft.dirty ? <SaveBar saving={draft.saving} onSave={() => void save()} onDiscard={draft.discard} /> : undefined}
    >
      <div className="settings-dialog-body">
        <nav className="settings-nav" aria-label="Settings sections">
          {nav.map((item) => (
            <a key={item.id} href={`#${item.id}`} aria-current={section === item.id ? 'true' : undefined} onClick={(event) => { event.preventDefault(); onSection(item.id); }}>
              <Icon name={item.icon} size={15} />{item.title}
            </a>
          ))}
        </nav>
        <div className="settings-page settings-dialog-content">
          {personal && (draft.s ? (
            <FieldsContext.Provider value={draft.fields}>
              <PersonalSection id={section as PersonalSectionId} s={draft.s} set={draft.set} />
            </FieldsContext.Provider>
          ) : <p className="settings-capability-note" role="status"><Lamp state="caution" pulse /> {draft.error || 'Loading your settings…'}</p>)}
          {section === 'tools' && <ToolsPage registry={registry} wsId={wsId} notify={notify} onRefresh={onRefreshTools} />}
          {section === 'account' && (me?.sign_in && me.via === 'session'
            ? <AccountSection me={me} setToasts={setToasts} />
            : (
              <section className="settings-section" data-settings-title="Account">
                <h2><Icon name="lock" size={16} />Account</h2>
                <p className="settings-section-intro">{me?.sign_in ? `Signed in as ${me.name} with an API key.` : 'This install has no sign-in: Companion runs for this PC’s one person. On a company server, people sign in with their company account and make API keys here.'}</p>
              </section>
            ))}
        </div>
      </div>
    </Dialog>
  );
}
