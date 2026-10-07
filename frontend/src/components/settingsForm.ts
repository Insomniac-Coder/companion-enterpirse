/** Updating one visible preference must preserve hidden and legacy preferences. */
export function updateSetting<T>(settings: T, path: string[], value: unknown): T {
  const copy = structuredClone(settings);
  let target: any = copy;
  for (const key of path.slice(0, -1)) {
    target[key] ??= {};
    target = target[key];
  }
  target[path[path.length - 1]] = value;
  return copy;
}

export function settingsSearchMatches(text: string, query: string): boolean {
  return text.toLowerCase().includes(query.trim().toLowerCase());
}

export function expertSectionOpen(manuallyOpen: boolean, text: string, query: string): boolean {
  return manuallyOpen || (!!query.trim() && settingsSearchMatches(text, query));
}

/** Only what differs in `after` from `before`, nested as in the settings: what a level (the
 *  company's, a group's) sets. A list is one value. */
export function changedSettings(before: any, after: any): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(after ?? {})) {
    const was = before?.[key];
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      const inner = changedSettings(was, value);
      if (Object.keys(inner).length) out[key] = inner;
    } else if (JSON.stringify(value) !== JSON.stringify(was)) {
      out[key] = value;
    }
  }
  return out;
}

/** `base` with `over` laid on top: a group's values over the company's. */
export function withValues(base: any, over: any): any {
  const out = structuredClone(base ?? {});
  for (const [key, value] of Object.entries(over ?? {})) {
    out[key] = value && typeof value === 'object' && !Array.isArray(value) ? withValues(out[key], value) : value;
  }
  return out;
}
