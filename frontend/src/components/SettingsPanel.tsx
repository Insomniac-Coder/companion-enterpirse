import { MODES, performanceMode, getCalibration, profileSummary, staleReason, type CalibrationStatus } from '../services/calibration';
import { contextSupportWarning } from '../services/contextSupport';
import { cacheConflict, flashAttentionRequired, FLASH_ATTENTION_CONFLICT_NOTE, FLASH_ATTENTION_REQUIRED_NOTE, quantizedCacheUnavailable, QUANTIZED_CACHE_UNAVAILABLE_NOTE } from '../services/cacheCompatibility';
import { Children, cloneElement, createContext, isValidElement, useContext, useEffect, useId, useRef, useState, type ReactElement, type ReactNode } from 'react';
import { getSettingFields, lockNote, type SettingFields } from '../services/settingsFields';
import { getSettings, putSettings, listModels, scanModels, getRuntimePolicy, type ModelMeta } from '../services/api';
import { pushToast, type Toast } from './Toasts';
import { defaultModelOptions, type ModelListState } from './settingsModels';
import { changedSettings, updateSetting } from './settingsForm';
import { PERMISSION_MODE_LABELS } from './permissionCopy';
import { Button, Lamp } from '../ui/primitives';
import { Icon, type IconName } from '../ui/Icon';

type SetPreference = (path: string[], value: unknown) => void;
type RuntimePolicy = Awaited<ReturnType<typeof getRuntimePolicy>>;

export function Num({ obj, k, set, id }: { obj: any; k: string; set: (value: number) => void; id?: string }) {
  return <input type="number" id={id} aria-label={id ? undefined : k.replace(/_/g, ' ')}
    step={['temperature', 'top_p', 'repeat_penalty'].includes(k) ? 'any' : 1}
    value={obj?.[k] ?? ''} onChange={(event) => set(Number(event.target.value))} />;
}

/** What this person may do with each field (the server's word); none on a screen without it. */
export const FieldsContext = createContext<SettingFields | null>(null);
/** On the dashboard: the lock control shown beside each field (`path`). */
export const LockContext = createContext<((path: string) => ReactNode) | null>(null);

/** Labels, controls, supplementary actions and help remain one indivisible row. A field this
 * person cannot change (`path` locked, or the company's) is disabled and says why. */
export function SettingField({ label, children, description, path }: { label: string; children: ReactNode; description?: ReactNode; path?: string }) {
  const fields = useContext(FieldsContext);
  const lock = useContext(LockContext);
  const note = lockNote(path ? fields?.[path] : undefined);
  const id = useId();
  const items = Children.toArray(children);
  const first = items[0];
  const control = isValidElement(first) && (first.type === Num || ['input', 'select', 'textarea'].includes(first.type as string));
  const checkbox = control && (first as ReactElement<{ type?: string }>).props.type === 'checkbox';
  if (control) items[0] = cloneElement(first as ReactElement<{ id?: string; 'aria-describedby'?: string }>, { id, ...(description ? { 'aria-describedby': `${id}-description` } : {}) });
  return <div className={`settings-field${checkbox ? ' settings-field--toggle' : ''}`}>
    {control ? <label className="settings-field-label" htmlFor={id}>{label}</label> : <span className="settings-field-label">{label}</span>}
    <div className="settings-field-control">{note ? <fieldset className="settings-field-locked" disabled>{items}</fieldset> : items}{lock && path ? lock(path) : null}</div>
    {note && <div className="settings-field-lock"><Icon name="lock" size={12} />{note}</div>}
    {description && <div className="settings-field-description" id={`${id}-description`}>{description}</div>}
  </div>;
}

export function HardwareOverrides({ settings, set }: { settings: any; set: SetPreference }) {
  if (performanceMode(settings) !== 'manual') return null;
  return <div className="settings-hardware-overrides">
    <p className="settings-capability-note">Manual values apply after Save and the next model load. Your previous values are kept when automatic management is on.</p>
    <div className="settings-fields">
      <SettingField label="CPU threads" path="hardware.cpu_threads"><Num obj={settings.hardware} k="cpu_threads" set={(value) => set(['hardware', 'cpu_threads'], value)} /></SettingField>
      <SettingField label="GPU layers" path="hardware.gpu_layers" description="Use −1 to let the runtime choose how many layers to place on the GPU."><Num obj={settings.hardware} k="gpu_layers" set={(value) => set(['hardware', 'gpu_layers'], value)} /></SettingField>
      <SettingField label="Flash attention" path="hardware.flash_attention" description={flashAttentionRequired(settings, performanceMode(settings)) ? FLASH_ATTENTION_REQUIRED_NOTE : cacheConflict(settings, performanceMode(settings)) ? <span className="settings-context-warning">{FLASH_ATTENTION_CONFLICT_NOTE}</span> : undefined}><input type="checkbox" className="switch" checked={!!settings.hardware?.flash_attention} disabled={flashAttentionRequired(settings, performanceMode(settings))} onChange={(event) => set(['hardware', 'flash_attention'], event.target.checked)} /></SettingField>
      <SettingField label="KV cache on GPU" path="hardware.kv_cache_gpu"><input type="checkbox" className="switch" checked={!!settings.hardware?.kv_cache_gpu} onChange={(event) => set(['hardware', 'kv_cache_gpu'], event.target.checked)} /></SettingField>
      <SettingField label="Prompt batch size" path="inference.batch_size"><Num obj={settings.inference} k="batch_size" set={(value) => set(['inference', 'batch_size'], value)} /></SettingField>
      <SettingField label="Prompt threads" path="hardware.threads_batch" description="Threads for reading prompts; 0 uses the CPU threads value. Prompts come in short bursts, so this can be higher than the generation threads without keeping the machine busy."><Num obj={settings.hardware} k="threads_batch" set={(value) => set(['hardware', 'threads_batch'], value)} /></SettingField>
      <SettingField label="Wait between operations" path="hardware.poll" description="Spin keeps worker threads busy-waiting for the next step (slightly faster, uses CPU while idle). Sleep lets them rest."><select value={settings.hardware?.poll === 0 ? 'sleep' : 'spin'} onChange={(event) => set(['hardware', 'poll'], event.target.value === 'sleep' ? 0 : 50)}><option value="spin">Spin (runtime default)</option><option value="sleep">Sleep</option></select></SettingField>
      <SettingField label="Priority" path="hardware.priority" description="Low lets other applications take the CPU first when they need it."><select value={String(settings.hardware?.priority ?? 0)} onChange={(event) => set(['hardware', 'priority'], Number(event.target.value))}><option value="0">Normal</option><option value="-1">Low</option></select></SettingField>
    </div>
  </div>;
}

/** What the selected mode means for the default model, in measured terms. */
export function PerformanceModeNote({ settings, status, forModel }: { settings: any; status: CalibrationStatus | null; forModel?: boolean }) {
  const mode = performanceMode(settings);
  const base = 'Applies at the next model load. Profiles are measured per model: calibrate a model from its details on the Models page.';
  if (mode === 'auto' || mode === 'manual') return <span>{base}</span>;
  if (!forModel && !settings?.general?.default_model) return <span>{base} Choose a default model to see what this profile does for it.</span>;
  const calibration = status?.calibration;
  if (!calibration) return <span>{base} The default model is not calibrated yet, so it will load with automatic settings until it is.</span>;
  const stale = staleReason(status);
  if (stale) return <span>{base} {stale}</span>;
  const profile = calibration.profiles.find((candidate) => candidate.name === mode);
  return <span>{base}{profile ? <span className="settings-mode-measured"> For {forModel ? 'this' : 'the default'} model: {profileSummary(profile)}.</span> : null}</span>;
}

export function RuntimeSummary({ policy, dirty }: { policy: RuntimePolicy; dirty: boolean }) {
  const configuration = (value: RuntimePolicy['next']) => <dl className="settings-runtime-values">
    <div><dt>Model architecture</dt><dd>{value.architecture && value.architecture !== 'unknown' ? value.architecture : 'Determined when a model is loaded'}</dd></div>
    <div><dt>Model weights</dt><dd>{value.weights_quantization && value.weights_quantization !== 'unknown' ? value.weights_quantization : 'Determined when a model is loaded'}</dd></div>
    <div><dt>Requested cache precision</dt><dd>{value.cache_type_k} keys / {value.cache_type_v} values</dd></div>
    <div><dt>Context window</dt><dd>{value.effective_context.toLocaleString()} tokens</dd></div>
    <div><dt>CPU threads</dt><dd>{value.threads === 0 ? 'Runtime managed' : value.threads}</dd></div>
    <div><dt>GPU layers</dt><dd>{value.gpu_layers === -1 ? 'Runtime chooses' : value.gpu_layers}</dd></div>
    <div><dt>Prompt batch</dt><dd>{value.batch_size === 0 ? 'Runtime default' : value.batch_size}</dd></div>
    <div><dt>Flash attention</dt><dd>{value.flash_attention}</dd></div>
    <div><dt>Cache placement</dt><dd>{value.kv_offload}</dd></div>
    {value.placement && value.placement !== 'unknown' && <div><dt>Planned placement</dt><dd>{value.placement === 'gpu' ? 'Whole model on the GPU' : value.placement === 'hybrid' ? 'Split between GPU and system RAM (slower)' : value.placement === 'oversubscribed' ? 'Does not fit GPU + RAM (may fail or page)' : value.placement === 'cpu' ? 'CPU only' : value.placement}</dd></div>}
    <div><dt>Speculative decoding</dt><dd>{value.speculative && value.speculative !== 'none' ? `${value.speculative} (drafts from context, lossless)` : 'off'}</dd></div>
    <div><dt>Cache chunk reuse</dt><dd>{value.cache_reuse ? `${value.cache_reuse.toLocaleString()}-token minimum` : 'off'}</dd></div>
  </dl>;
  return <details className="settings-runtime-details">
    <summary>View runtime configuration</summary>
    <p className="settings-capability-note">These are requested settings, not measurements of memory use. Model-file quantization describes the weights; it does not set cache precision.</p>
    {policy.active && <div className="settings-runtime-group"><h3>Loaded session</h3>{configuration(policy.active)}{policy.active.notes.length > 0 && <ul className="settings-runtime-notes">{policy.active.notes.map((note, index) => <li key={index}>{note}</li>)}</ul>}</div>}
    {!policy.active && <p className="settings-capability-note">No managed model session is loaded.</p>}
    <div className="settings-runtime-group"><h3>Next model load</h3>
      {dirty ? <p className="settings-capability-note">Save your changes to update the planned configuration.</p> : <>{configuration(policy.next)}{policy.next.notes.length > 0 && <ul className="settings-runtime-notes">{policy.next.notes.map((note, index) => <li key={index}>{note}</li>)}</ul>}</>}
    </div>
  </details>;
}

/** Where a screen reads and saves: the person's own settings (no scope), or a level the
 *  dashboard edits, the company's or a group's. */
export interface SettingsScope {
  load: () => Promise<any>;
  /** Saves what differs from the loaded values; returns the values to show after. */
  save: (changed: Record<string, unknown>, all: any) => Promise<any>;
  /** Which fields this level may set, and why not; every field when absent. */
  fields?: () => Promise<SettingFields>;
}

/** The settings a screen edits: a draft over what was loaded, saved through `scope` (or as the
 *  person's own). A screen keyed by its scope starts a fresh draft when the scope changes. */
export function useSettingsDraft(scope: SettingsScope | undefined, onError: (message: string) => void) {
  const [s, setS] = useState<any>(null);
  const [saved, setSaved] = useState<any>(null);
  const [error, setError] = useState('');
  const [fields, setFields] = useState<SettingFields | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const revision = useRef(0);
  useEffect(() => {
    let active = true;
    (scope ? scope.fields : getSettingFields)?.().then((next) => { if (active) setFields(next); }).catch(() => {});
    (scope?.load ?? getSettings)().then((value) => { if (active) { setS(value); setSaved(value); } }).catch((e) => { if (active) setError(e.message); });
    return () => { active = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => {
    const warn = (event: BeforeUnloadEvent) => { if (dirty) { event.preventDefault(); event.returnValue = ''; } };
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty]);
  const set: SetPreference = (path, value) => { revision.current++; setDirty(true); setS((previous: any) => updateSetting(previous, path, value)); };
  const discard = () => { revision.current++; setS(saved); setDirty(false); };
  /** The saved values, or null when saving failed (the reason went to `onError`). */
  const save = async (): Promise<any | null> => {
    if (saving) return null;
    setSaving(true);
    const at = revision.current;
    try {
      const next = scope ? await scope.save(changedSettings(saved, s), s) : await putSettings(s);
      if (at === revision.current) { setS(next); setSaved(next); setDirty(false); }
      return next;
    } catch (e) {
      onError(e instanceof Error ? e.message : 'Settings could not be saved.');
      return null;
    } finally {
      setSaving(false);
    }
  };
  return { s, set, dirty, saving, error, fields, save, discard };
}

export function SaveBar({ saving, blocked, onSave, onDiscard }: { saving: boolean; blocked?: string | null; onSave: () => void; onDiscard: () => void }) {
  return (
    <div className="save-bar" role="status">
      <Lamp state="caution" />
      <span>{blocked ?? 'Unsaved changes'}</span>
      <Button variant="ghost" size="sm" disabled={saving} onClick={onDiscard}>Discard</Button>
      <Button size="sm" loading={saving} disabled={!!blocked} title={blocked ?? undefined} onClick={onSave}>Save changes</Button>
    </div>
  );
}

/** A list of folders or addresses, one per line; empty lines are dropped when the field is left. */
function Lines({ value, set, placeholder }: { value: string[] | undefined; set: (next: string[]) => void; placeholder: string }) {
  const [text, setText] = useState((value ?? []).join('\n'));
  useEffect(() => { setText((value ?? []).join('\n')); }, [value?.join('\n')]);
  return <textarea rows={3} placeholder={placeholder} value={text} onChange={(event) => setText(event.target.value)} onBlur={() => set(text.split('\n').map((line) => line.trim()).filter(Boolean))} />;
}

/** The dashboard's settings: the parts of Companion the people running it decide. `startup`: which
 *  model loads when the server starts; `model`: a model's context, sampling, performance and hardware
 *  (the company's defaults, or one model's own). */
export type ServiceSection = 'startup' | 'model' | 'search' | 'rules' | 'privacy';

export default function ServiceSettings({ sections, scope, setToasts, readOnly, note, model, bare }: {
  sections: ServiceSection[];
  scope: SettingsScope;
  setToasts: React.Dispatch<React.SetStateAction<Toast[]>>;
  /** An auditor reads; nothing can be changed. */
  readOnly?: boolean;
  /** What this level is, shown above the fields. */
  note?: ReactNode;
  /** One model's own settings, not the defaults for every model. */
  model?: { id: string; name: string };
  /** Inside a dialog: no page frame. */
  bare?: boolean;
}) {
  const draft = useSettingsDraft(scope, (message) => pushToast(setToasts, 'error', message));
  const { s, set } = draft;
  const [models, setModels] = useState<ModelMeta[]>([]);
  const [modelListState, setModelListState] = useState<ModelListState>('loading');
  const [modelListNotice, setModelListNotice] = useState('');
  const [calibrationStatus, setCalibrationStatus] = useState<CalibrationStatus | null>(null);
  const [policy, setPolicy] = useState<RuntimePolicy | null>(null);
  const [policyError, setPolicyError] = useState('');
  const withModels = sections.includes('model') || sections.includes('startup');
  // Calibration and the runtime plan describe one model: this one, or the one loaded at startup.
  const defaultModelId: string = model?.id ?? s?.general?.default_model ?? '';

  const refreshModels = async (rescan = false) => {
    setModelListState('loading');
    setModelListNotice('');
    try {
      const scan = rescan ? await scanModels() : null;
      setModels(await listModels());
      setModelListState('ready');
      setModelListNotice(scan?.warnings.join(' ') ?? '');
    } catch (error) {
      setModelListState('error');
      setModelListNotice(error instanceof Error ? error.message : 'The model list could not be loaded.');
    }
  };
  const refreshPolicy = (modelId?: string) => getRuntimePolicy(modelId || undefined)
    .then((next) => { setPolicy(next); setPolicyError(''); })
    .catch((error) => setPolicyError(error instanceof Error ? error.message : 'Runtime configuration is unavailable.'));
  useEffect(() => { if (withModels) void refreshModels(); }, [withModels]);
  useEffect(() => { if (withModels && s) void refreshPolicy(defaultModelId); }, [withModels, !!s, defaultModelId]);
  useEffect(() => {
    if (!withModels || !defaultModelId) { setCalibrationStatus(null); return; }
    let current = true;
    getCalibration(defaultModelId).then((next) => { if (current) setCalibrationStatus(next); }).catch(() => { if (current) setCalibrationStatus(null); });
    return () => { current = false; };
  }, [withModels, defaultModelId]);

  const frame = (content: ReactNode) => bare ? <div className="settings-page">{content}</div> : <div className="page"><div className="page-inner"><div className="settings-page">{content}</div></div></div>;
  if (!s) return frame(<div className="panel empty-state" role="status">{draft.error ? <><Icon name="alertCircle" size={26} /><strong>Settings unavailable</strong><p>{draft.error}</p></> : <><Lamp state="caution" pulse /><p>Loading settings…</p></>}</div>);

  const conflict = withModels ? cacheConflict(s, performanceMode(s)) : null;
  const save = async () => {
    if (conflict) return;
    const next = await draft.save();
    if (!next) return;
    if (withModels) void refreshPolicy(model?.id ?? next.general?.default_model);
    pushToast(setToasts, 'success', model ? `Saved. ${model.name} uses them from its next load.` : 'Saved. People get the new values with their next request; model changes apply on the next model load.');
  };
  const modes = Object.keys(PERMISSION_MODE_LABELS) as (keyof typeof PERMISSION_MODE_LABELS)[];

  return <FieldsContext.Provider value={draft.fields}>{frame(<>
        {note && <p className="settings-capability-note">{note}</p>}
        <fieldset className="settings-readonly" disabled={readOnly}>
          {sections.includes('startup') && (
            <section className="settings-section" data-settings-title="Model loaded at startup">
              <h2><Icon name="power" size={16} />Model loaded at startup</h2><p className="settings-section-intro">The model the server loads when it starts, for everyone. People choose among the models running.</p>
              <div className="settings-fields">
                <SettingField label="Model loaded at startup" path="general.default_model" description={<><span>Loaded when the server starts. A model already running keeps running until it is switched on the Models page.</span><span className="settings-model-status" role={modelListState === 'error' ? 'alert' : 'status'}>{modelListState === 'loading' ? 'Looking for models…' : modelListState === 'error' ? `Could not refresh models. ${modelListNotice}` : modelListNotice || (models.length === 0 ? 'No models found. Add one on the Models page, then refresh.' : s.general?.default_model && !models.some((model) => model.id === s.general.default_model) ? 'The saved model is not installed any more. Choose another.' : '')}</span></>}>
                  <select value={s.general?.default_model ?? ''} disabled={modelListState === 'loading' && models.length === 0} onChange={(event) => set(['general', 'default_model'], event.target.value)}>{defaultModelOptions(models, s.general?.default_model ?? '', modelListState).map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}</select>
                  <button className="btn secondary" type="button" disabled={modelListState === 'loading'} onClick={() => void refreshModels(true)} aria-label={modelListState === 'error' ? 'Retry model discovery' : 'Refresh detected models'}>{modelListState === 'loading' ? 'Refreshing…' : modelListState === 'error' ? 'Retry' : 'Refresh'}</button>
                </SettingField>
              </div>
            </section>
          )}
          {sections.includes('model') && (
            <section className="settings-section settings-performance" data-settings-title={model ? model.name : 'Defaults for every model'}>
              <h2><Icon name="layers" size={16} />{model ? model.name : 'Defaults for every model'}</h2>
              <p className="settings-section-intro">{model ? 'This model\u2019s context, sampling, performance and hardware. Each field starts from the defaults for every model (Model server); what you change here applies to this model only, from its next load.' : 'What every model starts with: context, sampling, performance and hardware. A model\u2019s own Settings (on the Models page) can change any of them for that model.'}</p>
              <div className="settings-fields">
                <SettingField label="Context size" path="inference.context_size" description={<><span>The window to ask for. Applies on the next model load; the next setting decides what happens when it does not fit memory.</span>{contextSupportWarning(Number(s.inference?.context_size), models, s.general?.default_model) && <span className="settings-context-warning" role="status">{contextSupportWarning(Number(s.inference?.context_size), models, s.general?.default_model)}</span>}</>}><Num obj={s.inference} k="context_size" set={(value) => set(['inference', 'context_size'], value)} /></SettingField>
                <SettingField label="When the context size does not fit" path="runtime.context_fit" description="Fit it to memory keeps the whole model on the GPU by loading a smaller window, which is faster. Use the size as written keeps the window and lets part of the model run on the CPU, which is slower."><select value={s.runtime?.context_fit === 'requested' ? 'requested' : 'fit'} onChange={(event) => set(['runtime', 'context_fit'], event.target.value)}><option value="fit">Fit it to memory (faster)</option><option value="requested">Use the size as written (slower)</option></select></SettingField>
                <SettingField label="Temperature" path="inference.temperature"><Num obj={s.inference} k="temperature" set={(value) => set(['inference', 'temperature'], value)} /></SettingField>
                <SettingField label="Top-p" path="inference.top_p"><Num obj={s.inference} k="top_p" set={(value) => set(['inference', 'top_p'], value)} /></SettingField>
                <SettingField label="Top-k" path="inference.top_k"><Num obj={s.inference} k="top_k" set={(value) => set(['inference', 'top_k'], value)} /></SettingField>
                <SettingField label="Repeat penalty" path="inference.repeat_penalty"><Num obj={s.inference} k="repeat_penalty" set={(value) => set(['inference', 'repeat_penalty'], value)} /></SettingField>
                <SettingField label="Performance mode" path="runtime.mode" description={<PerformanceModeNote settings={s} status={calibrationStatus} forModel={!!model} />}>
                  <div className="settings-mode-options" role="radiogroup" aria-label="Performance mode">
                    {MODES.map((option) => (
                      <label key={option.value} className={`settings-mode-option${performanceMode(s) === option.value ? ' selected' : ''}`}>
                        <input type="radio" name="performance-mode" value={option.value} checked={performanceMode(s) === option.value} onChange={() => { set(['runtime', 'mode'], option.value); set(['runtime_auto'], option.value !== 'manual'); }} />
                        <strong>{option.label}</strong>
                        <span>{option.description}</span>
                      </label>
                    ))}
                  </div>
                </SettingField>
                <SettingField label="Speculative decoding" path="runtime.speculative" description="Auto drafts tokens that already appear in the context and verifies them in one step. The model still chooses every word. Replies that repeat the context (code edits, file rewrites, tool calls) finish faster; when nothing repeats it costs no measurable speed. Applies on the next model load."><select value={s.runtime?.speculative ?? 'auto'} onChange={(event) => set(['runtime', 'speculative'], event.target.value)}><option value="auto">Auto (draft from context)</option><option value="off">Off</option></select></SettingField>
                <SettingField label="KV cache precision" path="runtime.kv_cache" description={<><span>f16 is the compatibility default. q8_0 halves cache memory, which allows a larger context on the same GPU; it measured no slower with Flash Attention on. Applies on the next model load.</span>{quantizedCacheUnavailable(s, performanceMode(s)) && <span className="settings-context-warning" role={cacheConflict(s, performanceMode(s)) ? 'alert' : 'status'}>{cacheConflict(s, performanceMode(s)) ?? QUANTIZED_CACHE_UNAVAILABLE_NOTE}</span>}</>}><select value={s.runtime?.kv_cache ?? 'f16'} onChange={(event) => set(['runtime', 'kv_cache'], event.target.value)}><option value="f16">f16 (default)</option><option value="q8_0" disabled={quantizedCacheUnavailable(s, performanceMode(s))}>q8_0 (half the cache memory{quantizedCacheUnavailable(s, performanceMode(s)) ? '; needs Flash Attention' : ''})</option></select></SettingField>
              </div>
              <HardwareOverrides settings={s} set={set} />
              {policyError && <p className="settings-policy-error" role="alert">Could not read the runtime configuration. <button className="btn secondary sm" type="button" onClick={() => void refreshPolicy(defaultModelId)}>Retry</button></p>}
              {policy && !policyError && <RuntimeSummary policy={policy} dirty={draft.dirty} />}
            </section>
          )}

          {sections.includes('search') && (
            <section className="settings-section" data-settings-title="Web search">
              <h2><Icon name="globe" size={16} />Web search</h2><p className="settings-section-intro">Where searches go when someone turns search on for a message. Search requests leave the company.</p>
              <div className="settings-fields">
                <SettingField label="Search provider" path="search.provider" description={s.search?.provider === 'custom' ? 'The saved custom provider is not implemented. Choose a supported provider to use search.' : undefined}><select value={s.search?.provider ?? 'duckduckgo'} onChange={(event) => set(['search', 'provider'], event.target.value)}><option value="duckduckgo">DuckDuckGo (no key)</option><option value="brave">Brave (API key)</option>{s.search?.provider === 'custom' && <option value="custom">Saved custom provider — unavailable</option>}</select></SettingField>
                {s.search?.provider === 'brave' && <SettingField label="Brave API key" path="search.brave_key"><input type="password" autoComplete="off" value={s.search?.brave_key ?? ''} onChange={(event) => set(['search', 'brave_key'], event.target.value)} /></SettingField>}
                <SettingField label="Maximum search results" path="search.max_results"><Num obj={s.search} k="max_results" set={(value) => set(['search', 'max_results'], value)} /></SettingField>
                <SettingField label="Search timeout (seconds)" path="search.timeout_secs"><Num obj={s.search} k="timeout_secs" set={(value) => set(['search', 'timeout_secs'], value)} /></SettingField>
              </div>
            </section>
          )}

          {sections.includes('rules') && (
            <section className="settings-section" data-settings-title="Rules">
              <h2><Icon name="shield" size={16} />Rules</h2><p className="settings-section-intro">What people's work may touch. People cannot change these; a group's values replace the company's for its members.</p>
              <div className="settings-fields">
                <SettingField label="Most permissive mode allowed" path="agent.max_permission_mode" description="Nobody can choose a permission mode that allows more than this; a mode someone chose before counts as this one."><select value={s.agent?.max_permission_mode ?? 'auto'} onChange={(event) => set(['agent', 'max_permission_mode'], event.target.value)}>{modes.map((option) => <option key={option} value={option}>{PERMISSION_MODE_LABELS[option]}</option>)}</select></SettingField>
                <SettingField label="Allowed project folders" path="security.allowed_dirs" description="One folder per line. When any are listed, a project must be inside one of them. Empty: any folder."><Lines value={s.security?.allowed_dirs} placeholder="D:\Projects" set={(next) => set(['security', 'allowed_dirs'], next)} /></SettingField>
                <SettingField label="Blocked folders" path="security.blocked_dirs" description="One folder per line. No project may be inside one of these, even an allowed one."><Lines value={s.security?.blocked_dirs} placeholder="C:\Windows" set={(next) => set(['security', 'blocked_dirs'], next)} /></SettingField>
                <SettingField label="Internet access" path="network.policy" description="On request: web search when someone turns it on for a message. Selected sites: only the trusted addresses below. Off: nothing leaves the company."><select value={s.network?.policy ?? 'ask'} onChange={(event) => set(['network', 'policy'], event.target.value)}><option value="ask">On request</option><option value="selected">Selected sites only</option><option value="disabled">Off</option></select></SettingField>
                {s.network?.policy === 'selected' && <SettingField label="Trusted sites" path="network.trusted_hosts" description="One address per line; a site covers its subdomains."><Lines value={s.network?.trusted_hosts} placeholder="api.search.brave.com" set={(next) => set(['network', 'trusted_hosts'], next)} /></SettingField>}
                <SettingField label="Largest file attachment (MB)" path="files.max_attach_mb" description="1 to 50."><Num obj={s.files} k="max_attach_mb" set={(value) => set(['files', 'max_attach_mb'], value)} /></SettingField>
                <SettingField label="Largest image attachment (MB)" path="files.max_image_mb" description="1 to 50."><Num obj={s.files} k="max_image_mb" set={(value) => set(['files', 'max_image_mb'], value)} /></SettingField>
                <SettingField label="Longest command (minutes)" path="agent.command_timeout_secs" description="How long a command or project check may run before it is stopped."><input type="number" min={1} value={Math.round((s.agent?.command_timeout_secs ?? 1800) / 60)} onChange={(event) => set(['agent', 'command_timeout_secs'], Math.max(1, Number(event.target.value)) * 60)} /></SettingField>
              </div>
            </section>
          )}

          {sections.includes('privacy') && (
            <section className="settings-section" data-settings-title="Privacy and logging">
              <h2><Icon name="lock" size={16} />Privacy and logging</h2><p className="settings-section-intro">What the server keeps about the work done on it. The audit records are kept either way.</p>
              <div className="settings-fields">
                <SettingField label="Keep a record of model requests" path="privacy.record_model_requests" description="Stores what was sent to the model and what it returned for each reply and agent step, so a wrong or broken answer can be diagnosed. Limited to the most recent 300 requests. Records can contain file contents the assistant read."><input type="checkbox" className="switch" checked={s.privacy?.record_model_requests !== false} onChange={(event) => set(['privacy', 'record_model_requests'], event.target.checked)} /></SettingField>
                <SettingField label="Mask secrets in the logs" path="privacy.log_redaction" description="Bearer tokens, API keys, passwords in addresses and password= values are written as *** in the server's log files (and so in a logs zip)."><input type="checkbox" className="switch" checked={s.privacy?.log_redaction !== false} onChange={(event) => set(['privacy', 'log_redaction'], event.target.checked)} /></SettingField>
              </div>
            </section>
          )}
        </fieldset>
        {draft.dirty && !readOnly && <SaveBar saving={draft.saving} blocked={conflict ? 'Cannot save: an 8-bit KV cache needs Flash Attention' : null} onSave={() => void save()} onDiscard={draft.discard} />}
  </>)}</FieldsContext.Provider>;
}
