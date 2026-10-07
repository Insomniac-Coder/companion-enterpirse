// The servers that host models (per the owner): pick one to see what it is using. One server today,
// this one; when Companion runs on several, each appears here with its own readings.
import { useEffect, useState } from 'react';
import ResourcesPanel from './ResourcesPanel';
import { listServers, type Server } from '../services/admin';
import { Lamp } from '../ui/primitives';

type Notify = (kind: 'info' | 'success' | 'warning' | 'error', text: string) => void;

export default function ServersPage({ notify }: { notify: Notify }) {
  const [servers, setServers] = useState<Server[] | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  useEffect(() => {
    const load = () => listServers().then((found) => setServers(found.servers)).catch((e) => notify('error', e.message));
    void load();
    const timer = setInterval(() => { if (!document.hidden) void load(); }, 10000);
    return () => clearInterval(timer);
  }, [notify]);
  const shown = chosen ?? servers?.[0]?.id ?? null;

  return (
    <>
      <div className="page server-list-page">
        <div className="page-inner admin-page">
          {servers === null ? <p className="settings-capability-note">Looking for servers…</p> : (
            <div className="server-list" role="list" aria-label="Servers">
              {servers.map((server) => (
                <button type="button" role="listitem" key={server.id} className="server-card" aria-current={shown === server.id ? 'true' : undefined} onClick={() => setChosen(server.id)}>
                  <span className="server-card-head">
                    <Lamp state={server.state === 'serving' ? 'ready' : 'off'} />
                    <strong>{server.name}</strong>
                    <span className="server-state">{server.state === 'serving' ? 'Serving' : 'Idle'}</span>
                  </span>
                  <span className="server-models">{server.models.length ? server.models.map((model) => model.name).join(', ') : 'No model loaded'}</span>
                  <span className="server-spec">{[server.cpu, server.gpus.map((gpu) => `${gpu.model}${gpu.vram_gb ? ` ${gpu.vram_gb.toFixed(0)} GB` : ''}`).join(', '), server.ram_gb ? `${server.ram_gb.toFixed(0)} GB RAM` : '', server.os].filter(Boolean).join(' · ')}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
      {shown && <ResourcesPanel notify={notify} />}
    </>
  );
}
