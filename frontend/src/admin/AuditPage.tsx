import { useEffect, useMemo, useState } from 'react';
import { ACTION_LABELS, auditRecords, auditWhat, auditWho, listPeople, serverTime, type AuditRecord, type Person } from '../services/admin';
import { Button } from '../ui/primitives';

type Notify = (kind: 'info' | 'success' | 'warning' | 'error', text: string) => void;

const PAGE = 100;

/** The audit records, newest first, with filters over what is loaded. The full viewer (filters on
 *  the server, export, the tamper check) is Phase 7's. */
export default function AuditPage({ notify }: { notify: Notify }) {
  const [records, setRecords] = useState<AuditRecord[] | null>(null);
  const [people, setPeople] = useState<Map<string, Person>>(new Map());
  const [more, setMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [action, setAction] = useState('');
  const [who, setWho] = useState('');
  const [text, setText] = useState('');

  async function load(before?: number) {
    setLoading(true);
    try {
      const page = (await auditRecords(before, PAGE)).records;
      setRecords((kept) => (before ? [...(kept ?? []), ...page] : page));
      setMore(page.length === PAGE);
    } catch (e: any) {
      notify('error', e.message);
      setRecords((kept) => kept ?? []);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void load();
    listPeople().then((list) => setPeople(new Map(list.map((person) => [person.id, person])))).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const shown = useMemo(() => (records ?? []).filter((record) => {
    if (action && record.action !== action) return false;
    if (who && record.user_id !== who) return false;
    if (text.trim()) {
      const haystack = `${auditWhat(record)} ${record.target} ${record.allowed_by} ${record.outcome} ${record.model} ${record.address} ${record.device}`.toLowerCase();
      if (!haystack.includes(text.trim().toLowerCase())) return false;
    }
    return true;
  }), [records, action, who, text]);

  const someone = [...new Set((records ?? []).map((record) => record.user_id))];

  return (
    <div className="page">
      <div className="page-inner admin-page admin-wide">
        <div className="admin-inline-form admin-filters">
          <select aria-label="Action" value={action} onChange={(event) => setAction(event.target.value)}>
            <option value="">Every action</option>
            {Object.entries(ACTION_LABELS).map(([key, label]) => <option key={key} value={key}>{label}</option>)}
          </select>
          <select aria-label="Person" value={who} onChange={(event) => setWho(event.target.value)}>
            <option value="">Everyone</option>
            {someone.map((id) => <option key={id} value={id}>{auditWho({ user_id: id } as AuditRecord, people)}</option>)}
          </select>
          <input aria-label="Search the records" placeholder="Search tool, query, route, address" value={text} onChange={(event) => setText(event.target.value)} />
          <Button size="sm" variant="ghost" icon="refresh" disabled={loading} onClick={() => void load()}>Newest</Button>
        </div>
        {records === null ? <p className="settings-capability-note">Loading the records…</p> : shown.length === 0 ? (
          <p className="settings-capability-note">{records.length === 0 ? 'No records yet. Every model request, tool call, web search, approval, change and sign-in adds one.' : 'No loaded record matches. Clear a filter, or load older records.'}</p>
        ) : (
          <table className="admin-table admin-audit">
            <thead><tr><th>When</th><th>Who</th><th>Action</th><th>What</th><th>Model and tokens</th><th>How it was allowed</th><th>Outcome</th></tr></thead>
            <tbody>
              {shown.map((record) => (
                <tr key={record.seq}>
                  <td className="admin-mono">{serverTime(record.at)}</td>
                  <td title={[record.via, record.address, record.device].filter(Boolean).join(' · ')}>
                    <strong>{auditWho(record, people)}</strong>
                    <small>{record.via}{record.address ? ` · ${record.address}` : ''}</small>
                  </td>
                  <td>{ACTION_LABELS[record.action] ?? record.action}</td>
                  <td className="admin-mono admin-what" title={auditWhat(record)}>{auditWhat(record)}</td>
                  <td className="admin-mono">{record.model ? `${record.model}` : ''}{record.prompt_tokens || record.generated_tokens ? ` ${record.prompt_tokens} in · ${record.generated_tokens} out` : ''}</td>
                  <td>{record.allowed_by}</td>
                  <td className="admin-mono">{record.outcome}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {more && <Button variant="ghost" icon="history" disabled={loading} onClick={() => void load(records?.[records.length - 1]?.seq)}>Load older records</Button>}
      </div>
    </div>
  );
}
