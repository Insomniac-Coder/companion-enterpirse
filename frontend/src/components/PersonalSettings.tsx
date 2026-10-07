// Each person's own settings (Phase 1 task 9, after the owner's review): how Companion looks and
// works for them, as ChatGPT and Claude lay out a person's settings. The same sections appear in
// Companion's Settings dialog (the person's own choices) and on the dashboard's People's defaults
// page (the company's or a group's defaults, with locks). Everything about running the service is
// the dashboard's (SettingsPanel's ServiceSettings).
import { SettingField, Num } from './SettingsPanel';
import { PERMISSION_MODE_DESCRIPTIONS, PERMISSION_MODE_LABELS, PROJECT_BOUNDARY_DESCRIPTION, SEARCH_PERMISSION_DESCRIPTION } from './permissionCopy';
import { modesWithin } from '../services/workbench';
import { Icon, type IconName } from '../ui/Icon';

export type PersonalSectionId = 'general' | 'chats' | 'memory' | 'agents' | 'keyboard';

export const PERSONAL_SECTIONS: { id: PersonalSectionId; title: string; icon: IconName; intro: string }[] = [
  { id: 'general', title: 'General', icon: 'sun', intro: 'How Companion looks.' },
  { id: 'chats', title: 'Chats', icon: 'chat', intro: 'How answers are written, and what each one shows.' },
  { id: 'memory', title: 'Memory and context', icon: 'history', intro: 'What happens when a conversation fills the model’s context.' },
  { id: 'agents', title: 'Code and agents', icon: 'code', intro: 'What an agent may do in your projects without asking you first.' },
  { id: 'keyboard', title: 'Keyboard', icon: 'hash', intro: 'Shortcuts.' },
];

type SetPreference = (path: string[], value: unknown) => void;

export function PersonalSection({ id, s, set }: { id: PersonalSectionId; s: any; set: SetPreference }) {
  const meta = PERSONAL_SECTIONS.find((section) => section.id === id)!;
  const modes = Object.keys(PERMISSION_MODE_LABELS) as (keyof typeof PERMISSION_MODE_LABELS)[];
  const mode = (s.agent?.permission_mode ?? (s.agent?.autonomous_enabled ? 'auto' : 'ask')) as keyof typeof PERMISSION_MODE_DESCRIPTIONS;
  return (
    <section className="settings-section" data-settings-title={meta.title}>
      <h2><Icon name={meta.icon} size={16} />{meta.title}</h2>
      <p className="settings-section-intro">{meta.intro}</p>
      {id === 'general' && (
        <div className="settings-fields">
          <SettingField label="Theme" path="appearance.theme"><select value={s.appearance?.theme ?? s.general?.theme ?? 'dark'} onChange={(event) => set(['appearance', 'theme'], event.target.value)}><option value="dark">Dark</option><option value="light">Light</option><option value="system">Match system</option></select></SettingField>
          <SettingField label="Density" path="appearance.density"><select value={s.appearance?.density ?? 'comfortable'} onChange={(event) => set(['appearance', 'density'], event.target.value)}><option value="comfortable">Comfortable</option><option value="compact">Compact</option></select></SettingField>
          <SettingField label="Reduce motion" path="appearance.reduce_motion"><input type="checkbox" className="switch" checked={!!s.appearance?.reduce_motion} onChange={(event) => set(['appearance', 'reduce_motion'], event.target.checked)} /></SettingField>
        </div>
      )}
      {id === 'chats' && (
        <div className="settings-fields">
          <SettingField label="Reasoning on by default" path="reasoning.default_on" description="Lets the model think before it answers; how it thinks depends on the model."><input type="checkbox" className="switch" checked={!!s.reasoning?.default_on} onChange={(event) => set(['reasoning', 'default_on'], event.target.checked)} /></SettingField>
          <SettingField label="Reasoning budget" path="reasoning.budget"><select value={s.reasoning?.budget ?? 'automatic'} onChange={(event) => set(['reasoning', 'budget'], event.target.value)}><option value="automatic">Automatic</option><option value="low">Low</option><option value="medium">Medium</option><option value="high">High</option></select></SettingField>
          <SettingField label="Show generation speed" path="diagnostics.show_generation_speed"><input type="checkbox" className="switch" checked={s.diagnostics?.show_generation_speed ?? true} onChange={(event) => set(['diagnostics', 'show_generation_speed'], event.target.checked)} /></SettingField>
          <SettingField label="Show detailed response metrics" path="diagnostics.show_detailed_metrics"><input type="checkbox" className="switch" checked={s.diagnostics?.show_detailed_metrics ?? false} onChange={(event) => set(['diagnostics', 'show_detailed_metrics'], event.target.checked)} /></SettingField>
        </div>
      )}
      {id === 'memory' && (
        <div className="settings-fields">
          <SettingField label="Automatic compaction" path="memory.auto_compact" description="When the context fills up, older messages and agent steps are summarized so nothing is silently dropped. An agent run pauses between steps while this happens and then resumes; a chat reply starts once it is done. Original messages stay saved."><select value={s.memory?.auto_compact === 'off' ? 'off' : 'automatic'} onChange={(event) => set(['memory', 'auto_compact'], event.target.value)}><option value="automatic">Automatic</option><option value="off">Off</option></select></SettingField>
          <SettingField label="Compact at (% of usable context)" path="memory.compact_at_pct" description="50–98. Usable context is the model's window minus the room kept for its next reply."><Num obj={{ compact_at_pct: s.memory?.compact_at_pct ?? 90 }} k="compact_at_pct" set={(value) => set(['memory', 'compact_at_pct'], value)} /></SettingField>
          <SettingField label="Recent messages kept after compaction" path="memory.compaction_keep_turns"><Num obj={s.memory} k="compaction_keep_turns" set={(value) => set(['memory', 'compaction_keep_turns'], value)} /></SettingField>
        </div>
      )}
      {id === 'agents' && (
        <div className="settings-fields">
          <SettingField label="Permission mode" path="agent.permission_mode" description={`${PERMISSION_MODE_DESCRIPTIONS[mode] ?? PERMISSION_MODE_DESCRIPTIONS.ask} ${PROJECT_BOUNDARY_DESCRIPTION} Shift+Tab in a code task cycles it.`}><select value={mode} onChange={(event) => { set(['agent', 'permission_mode'], event.target.value); set(['agent', 'autonomous_enabled'], event.target.value === 'auto'); }}>{modes.map((option) => <option key={option} value={option} disabled={!modesWithin([option], s.agent?.max_permission_mode ?? 'auto').length}>{PERMISSION_MODE_LABELS[option]}</option>)}</select></SettingField>
          <SettingField label="Agent web searches" path="search.autonomous" description={SEARCH_PERMISSION_DESCRIPTION}><select value={s.search?.autonomous ?? 'ask'} onChange={(event) => set(['search', 'autonomous'], event.target.value)}><option value="ask">Ask unless Auto mode is on</option><option value="allow">Allow agent searches</option><option value="deny">Do not allow agent searches</option></select></SettingField>
        </div>
      )}
      {id === 'keyboard' && (
        <div className="settings-fields">
          <SettingField label="Command palette shortcut" path="keyboard.command_palette" description="Other shortcuts: Ctrl+1 Chats, Ctrl+2 Code tasks, Esc close, Enter send, Shift+Enter new line, Shift+Tab permission mode."><input value={s.keyboard?.command_palette ?? 'ctrl+k'} onChange={(event) => set(['keyboard', 'command_palette'], event.target.value)} /></SettingField>
        </div>
      )}
    </section>
  );
}
