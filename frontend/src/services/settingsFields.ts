// What a person may do with each setting (Phase 1, task 6): the server says which fields are theirs
// to change, which the company or a group locked, and why.

export interface SettingFieldInfo {
  kind: 'personal' | 'policy' | 'machine';
  /** "company", a group's name, or null. */
  locked_by: string | null;
  reason: string;
  /** Whether this person can change it here. */
  editable: boolean;
}

export type SettingFields = Record<string, SettingFieldInfo>;

export async function getSettingFields(): Promise<SettingFields> {
  const r = await fetch('/api/settings/fields');
  if (!r.ok) throw new Error(`request failed: ${r.status}`);
  return r.json();
}

/** The note beside a field this person cannot change, or null when they can. */
export function lockNote(field: SettingFieldInfo | undefined): string | null {
  if (!field || field.editable) return null;
  const reason = field.reason.trim() ? `: ${field.reason.trim()}` : '';
  if (field.locked_by === 'company') return `Locked by your company${reason}`;
  if (field.locked_by) return `Locked by ${field.locked_by}${reason}`;
  return field.kind === 'personal' ? 'Set for you' : 'Set by your company';
}

/** Why `file` is over the person's attachment limits (their company's or group's), or null. */
export function attachmentTooLarge(file: { name: string; size: number; type: string }, limits: { max_attach_mb: number; max_image_mb: number } | undefined): string | null {
  if (!limits) return null;
  const image = file.type.startsWith('image/');
  const limit = image ? limits.max_image_mb : limits.max_attach_mb;
  if (file.size <= limit * 1_000_000) return null;
  return `${file.name} is ${(file.size / 1e6).toFixed(1)} MB; ${image ? 'images' : 'files'} can be up to ${limit} MB here.`;
}
