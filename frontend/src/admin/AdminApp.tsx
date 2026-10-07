// The dashboard (Phase 1, task 9): the people running Companion for everyone. A platform admin
// manages people and roles, company and group settings with their locks, models and the model
// server; an auditor reads people, settings and the audit records. Shares its parts (src/ui,
// src/services, the settings screen) with the user app.
import { Fragment, useCallback, useEffect, useState } from 'react';
import Toasts, { pushToast, type Toast } from '../components/Toasts';
import Rig from './Rig';
import ModelsPage from './ModelsPage';
import RuntimePage from './RuntimePage';
import ResourcesPanel from './ResourcesPanel';
import PeoplePage from './PeoplePage';
import { PeopleDefaultsPage, ServiceSettingsPage } from './SettingsPages';
import AuditPage from './AuditPage';
import {
  agentRuns, deleteModel, inferenceStart, inferenceStatus, listDownloads, listModels, loadModel, stopAgent, systemInfo, unloadModels,
  type DownloadInfo, type InferenceStatus, type ModelMeta,
} from '../services/api';
import { isPlatformAdmin, signOut, type Me } from '../services/account';
import { DASHBOARD_PAGES, pageFromHash, type DashboardPage } from '../services/admin';
import { serverUrl } from '../services/server.ts';
import { firstLoadNotice } from '../services/tooling';
import { selectAvailableModel } from '../services/workbench';
import { Button, Dialog, IconButton, Lamp } from '../ui/primitives';
import { Icon, type IconName } from '../ui/Icon';

type Confirm = { title: string; body: string; action: string; icon?: IconName; onConfirm: () => void };

const DESCRIPTIONS: Record<DashboardPage, string> = {
  people: 'Who uses Companion, their groups, and the roles that let them run it',
  defaults: 'What people start with in Companion’s Settings, for everyone or a group, and what they may not change',
  rules: 'What people’s work may touch: folders, internet access, attachments, commands, permission modes',
  'model-server': 'The model everyone uses: its context, sampling and how the hardware runs it',
  search: 'Where web searches go',
  privacy: 'What the server keeps about the work done on it',
  audit: 'Who did what, from where, with which model, and how it was allowed',
  models: 'Load, inspect and add the models everyone uses',
  system: 'The model server, health checks and speed',
  resources: 'Live processor, memory and graphics readings for the server',
};

export default function AdminApp({ me }: { me?: Me }) {
  const admin = isPlatformAdmin(me);
  const auditor = admin || !!me?.roles?.includes('auditor');
  const [page, setPage] = useState<DashboardPage>(() => pageFromHash(window.location.hash, admin));
  const [toasts, setToasts] = useState<Toast[]>([]);
  const notify = useCallback((kind: Toast['kind'], text: string) => pushToast(setToasts, kind, text), []);
  const dismissToast = useCallback((id: number) => setToasts((items) => items.filter((toast) => toast.id !== id)), []);

  // Models and the model server: a platform admin's, moved here from the user app as they were.
  const [models, setModels] = useState<ModelMeta[]>([]);
  const [modelId, setModelId] = useState('');
  const [downloads, setDownloads] = useState<DownloadInfo[]>([]);
  const [inf, setInf] = useState<InferenceStatus | null>(null);
  const [sys, setSys] = useState<any>(null);
  const [backendUp, setBackendUp] = useState<boolean | null>(null);
  const [loadingModel, setLoadingModel] = useState(false);
  const [guard, setGuard] = useState<{ kind: 'load' | 'start'; id: string; detail: string } | null>(null);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [mobileNav, setMobileNav] = useState(false);

  useEffect(() => {
    const onHash = () => setPage(pageFromHash(window.location.hash, admin));
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, [admin]);

  async function refreshModels() {
    try {
      const next = await listModels();
      setModels(next);
      setModelId((current) => selectAvailableModel(next, current, ''));
    } catch { /* the health check reports an unreachable server */ }
  }

  async function refreshDownloads() {
    try { setDownloads(await listDownloads()); } catch { /* as above */ }
  }

  useEffect(() => {
    if (!admin) return;
    const check = () => inferenceStatus().then((state) => { setInf(state); setBackendUp(true); }).catch(() => setBackendUp(false));
    void refreshModels();
    void refreshDownloads();
    check();
    systemInfo().then(setSys).catch(() => setSys(null));
    const downloadsTimer = setInterval(() => void refreshDownloads(), 2000);
    const statusTimer = setInterval(() => { if (!document.hidden) { void refreshModels(); check(); } }, 10000);
    return () => { clearInterval(downloadsTimer); clearInterval(statusTimer); };
  }, [admin]);

  /** Load or start, stopping first when an agent is mid-step and the server says so (409). */
  async function guardedSwitch(kind: 'load' | 'start', id: string, force: boolean) {
    setLoadingModel(true);
    try {
      const checking = firstLoadNotice(models.find((model) => model.id === id));
      if (checking) notify('info', checking);
      const started: { notices?: string[]; tooling_notice?: string | null; tooling?: { can_write?: boolean } | null } | undefined = kind === 'load'
        ? await loadModel(id, force)
        : await inferenceStart(id, force);
      await refreshModels();
      const status = await inferenceStatus();
      setInf(status);
      const name = models.find((model) => model.id === id)?.name ?? id;
      if (!force) notify('success', kind === 'load' ? `${name} is loaded and ready.` : 'Inference started.');
      const notices = started?.notices ?? [];
      for (const notice of notices) notify('warning', notice);
      if (started?.tooling_notice) notify(started.tooling?.can_write === false || !started.tooling ? 'warning' : 'info', started.tooling_notice);
      if (status.runtime_notice && !notices.includes(status.runtime_notice)) notify('info', status.runtime_notice);
    } catch (e: any) {
      if (e?.status === 409 && !force) setGuard({ kind, id, detail: e.message });
      else notify('error', e.message);
    } finally {
      setLoadingModel(false);
    }
  }

  const running = () => (inf?.running ? models.find((model) => model.loaded) : undefined);

  function requestLoad(id: string) {
    const current = running();
    const target = models.find((model) => model.id === id);
    if (current?.id === id) return;
    if (current) {
      setConfirm({
        title: `Switch to ${target?.name ?? id}?`,
        body: `${current.name} is unloaded first, so it stops answering everyone while ${target?.name ?? 'the new model'} loads. Conversations stay as they are; each one's next reply rebuilds its context with the new model.`,
        action: 'Switch model',
        icon: 'refresh',
        onConfirm: () => { setModelId(id); void guardedSwitch('load', id, false); },
      });
      return;
    }
    setModelId(id);
    void guardedSwitch('load', id, false);
  }

  async function unload() {
    setLoadingModel(true);
    try {
      await unloadModels();
      await refreshModels();
      setInf(await inferenceStatus());
      notify('info', 'Model ejected. Its memory is free again.');
    } catch (e: any) {
      notify('error', e?.message ?? 'Could not unload the model.');
    } finally {
      setLoadingModel(false);
    }
  }

  function requestEject() {
    setConfirm({
      title: `Eject ${running()?.name ?? 'the model'}?`,
      body: 'This frees its memory. Nobody gets an answer until a model is loaded again.',
      action: 'Eject model',
      icon: 'eject',
      onConfirm: () => void unload(),
    });
  }

  function requestReload() {
    setConfirm({
      title: `Reload ${running()?.name ?? 'the model'}?`,
      body: 'It is unavailable to everyone for a moment while it loads again. Use this if replies have become stuck or settings changed.',
      action: 'Reload model',
      icon: 'refresh',
      onConfirm: () => {
        void (async () => {
          try { await unloadModels(); } catch { /* the load below reports the useful error */ }
          await guardedSwitch('load', modelId, false);
        })();
      },
    });
  }

  function confirmDelete(model: ModelMeta) {
    setConfirm({
      title: `Delete ${model.name}?`,
      body: 'This permanently removes the model file from the models folder. Conversations that used it are kept.',
      action: 'Delete model',
      icon: 'trash',
      onConfirm: () => { deleteModel(model.id).then(() => { notify('success', `Deleted ${model.name}.`); void refreshModels(); }).catch((e) => notify('error', e.message)); },
    });
  }

  async function guardWait() {
    if (!guard) return;
    const g = guard;
    setGuard(null);
    notify('info', 'Waiting for the agents to reach a safe point…');
    for (let i = 0; i < 120; i++) {
      try {
        const runs = await agentRuns();
        if (!runs.some((run) => !['COMPLETED', 'FAILED', 'CANCELLED'].includes(run.state))) break;
      } catch { /* keep waiting */ }
      await new Promise((resolve) => setTimeout(resolve, 2000));
    }
    void guardedSwitch(g.kind, g.id, false);
  }

  async function guardStop() {
    if (!guard) return;
    const g = guard;
    setGuard(null);
    try {
      for (const run of await agentRuns()) {
        if (!['COMPLETED', 'FAILED', 'CANCELLED'].includes(run.state)) await stopAgent(run.id).catch(() => {});
      }
      await guardedSwitch(g.kind, g.id, true);
    } catch (e: any) {
      notify('error', e.message);
    }
  }

  function go(next: DashboardPage) {
    window.location.hash = next;
    setPage(next);
    setMobileNav(false);
  }

  if (!auditor) {
    return (
      <div className="dashboard-refused">
        <Icon name="lock" size={22} />
        <h1>The dashboard is for the people running Companion</h1>
        <p>A platform admin or an auditor of this server can open it. Ask a platform admin if you need a role.</p>
        <Button variant="primary" icon="chat" onClick={() => { window.location.href = serverUrl('/'); }}>Back to Companion</Button>
      </div>
    );
  }

  const pages = DASHBOARD_PAGES.filter((item) => admin || item.auditor);
  const current = DASHBOARD_PAGES.find((item) => item.id === page)!;
  const lamp = backendUp === false ? 'error' : loadingModel ? 'caution' : inf?.running ? 'ready' : 'off';

  return (
    <div className={`shell no-right dashboard${mobileNav ? ' mobile-nav' : ''}`}>
      {mobileNav && <button type="button" className="nav-scrim" aria-label="Close navigation" onClick={() => setMobileNav(false)} />}
      <aside className="sidebar" aria-label="Dashboard">
        <div className="sb-top">
          <div className="sb-brand">
            <div className="sb-wordmark">
              <span className="tally-mark" aria-hidden="true"><Lamp state={lamp} /></span>
              <strong>Companion</strong>
              <span className="dashboard-tag">Dashboard</span>
            </div>
          </div>
        </div>
        <nav className="sb-list dashboard-nav" aria-label="Dashboard pages">
          {pages.map((item, index) => (
            <Fragment key={item.id}>
              {(index === 0 || pages[index - 1].group !== item.group) && <div className="sb-section">{item.group}</div>}
              <button type="button" className="sb-link" aria-current={page === item.id ? 'page' : undefined} onClick={() => go(item.id)}>
                <Icon name={item.icon as IconName} size={16} />
                <span>{item.label}</span>
              </button>
            </Fragment>
          ))}
        </nav>
        <nav className="sb-utility" aria-label="Leave the dashboard">
          <a className="sb-link" href={serverUrl('/')}><Icon name="chat" size={16} /><span>Back to Companion</span></a>
          {me?.sign_in && (
            <div className="sb-person" title={me.email || me.name}>
              <span className="sb-person-initial" aria-hidden="true">{(me.name || '?').slice(0, 1).toUpperCase()}</span>
              <span className="sb-person-name">{me.name}</span>
              <IconButton icon="power" label={`Sign out ${me.name}`} size="sm" tipSide="top" onClick={() => { void signOut().then(() => { window.location.href = serverUrl('/admin/'); }, () => notify('error', 'Sign-out did not finish. Try again.')); }} />
            </div>
          )}
        </nav>
        {admin && (
          <Rig
            models={models}
            modelId={modelId}
            inf={inf}
            backendUp={backendUp}
            loadingModel={loadingModel}
            activity="idle"
            liveTps={null}
            lastTps={null}
            collapsed={false}
            onSelect={(id) => (running() ? requestLoad(id) : setModelId(id))}
            canManage
            onLoad={requestLoad}
            onUnload={requestEject}
            onReload={requestReload}
            onOpenModels={() => go('models')}
            onOpenResources={() => go('resources')}
            notify={(kind, text) => notify(kind, text)}
          />
        )}
      </aside>

      <div className="center">
        <Toasts toasts={toasts} dismiss={dismissToast} />
        <header className="head">
          <div className="head-mobile">
            <IconButton icon="menu" label="Open navigation" tip={false} aria-expanded={mobileNav} onClick={() => setMobileNav((open) => !open)} />
          </div>
          <div className="head-title">
            <h1>{current.label}</h1>
            <div className="head-sub"><span>{DESCRIPTIONS[page]}</span></div>
          </div>
        </header>
        {page === 'people' && <PeoplePage canChange={admin} me={me} notify={notify} />}
        {page === 'defaults' && <PeopleDefaultsPage canChange={admin} signIn={!!me?.sign_in} setToasts={setToasts} notify={notify} />}
        {page === 'rules' && <ServiceSettingsPage key="rules" section="rules" canChange={admin} signIn={!!me?.sign_in} setToasts={setToasts} notify={notify} />}
        {page === 'model-server' && admin && <ServiceSettingsPage key="models" section="models" canChange={admin} signIn={!!me?.sign_in} setToasts={setToasts} notify={notify} />}
        {page === 'search' && <ServiceSettingsPage key="search" section="search" canChange={admin} signIn={!!me?.sign_in} setToasts={setToasts} notify={notify} />}
        {page === 'privacy' && <ServiceSettingsPage key="privacy" section="privacy" canChange={admin} signIn={!!me?.sign_in} setToasts={setToasts} notify={notify} />}
        {page === 'audit' && <AuditPage notify={notify} />}
        {page === 'models' && admin && (
          <ModelsPage
            models={models}
            loadingModel={loadingModel}
            downloads={downloads}
            notify={notify}
            onLoad={requestLoad}
            onDelete={confirmDelete}
            refreshModels={refreshModels}
            refreshDownloads={refreshDownloads}
          />
        )}
        {page === 'system' && admin && (
          <RuntimePage
            inf={inf}
            sys={sys}
            modelId={modelId}
            modelName={models.find((model) => model.id === modelId)?.name}
            backendUp={backendUp}
            notify={notify}
            onStart={() => void guardedSwitch('start', modelId, false)}
            setInf={setInf}
            setSys={setSys}
          />
        )}
        {page === 'resources' && admin && <ResourcesPanel notify={notify} />}
      </div>

      {guard && (
        <Dialog
          role="alertdialog"
          icon="alert"
          title="Switch models while an agent is working?"
          description={guard.detail}
          onClose={() => setGuard(null)}
          footer={<>
            <Button variant="ghost" onClick={() => setGuard(null)}>Cancel</Button>
            <Button onClick={() => void guardWait()}>Wait for the current step</Button>
            <Button variant="danger" icon="stop" onClick={() => void guardStop()}>Stop agents and switch</Button>
          </>}
        />
      )}
      {confirm && (
        <Dialog
          size="sm"
          role="alertdialog"
          icon={confirm.icon}
          title={confirm.title}
          description={confirm.body}
          onClose={() => setConfirm(null)}
          footer={<>
            <Button variant="ghost" onClick={() => setConfirm(null)}>Cancel</Button>
            <Button variant="primary" onClick={() => { const run = confirm.onConfirm; setConfirm(null); run(); }}>{confirm.action}</Button>
          </>}
        />
      )}
    </div>
  );
}
