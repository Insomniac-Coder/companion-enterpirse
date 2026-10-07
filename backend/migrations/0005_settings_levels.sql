-- Phase 1, task 6: settings in three levels. The company's settings stay in `settings`
-- ('app_settings'); a group can set its own values for the fields that allow it, a person their own
-- choices; a field can be locked for the company or for a group.

CREATE TABLE group_settings (
    group_id TEXT PRIMARY KEY REFERENCES groups(id) ON DELETE CASCADE,
    -- Only the fields this group sets, nested as in the company's settings.
    settings JSONB NOT NULL DEFAULT '{}',
    -- When a person's groups disagree, the lowest number wins.
    priority INTEGER NOT NULL DEFAULT 100,
    updated_by TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE user_settings (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    -- Only the fields this person chose.
    settings JSONB NOT NULL DEFAULT '{}',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE setting_locks (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- A field, as its dotted path: "appearance.theme".
    path TEXT NOT NULL,
    -- None: locked for the whole company; otherwise for this group's members.
    group_id TEXT REFERENCES groups(id) ON DELETE CASCADE,
    -- Shown beside the locked field.
    reason TEXT NOT NULL DEFAULT '',
    set_by TEXT NOT NULL DEFAULT '',
    set_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX setting_locks_once ON setting_locks (path, COALESCE(group_id, ''));
