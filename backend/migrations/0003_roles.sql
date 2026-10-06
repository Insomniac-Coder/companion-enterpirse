-- Phase 1, task 4: groups and roles. Every signed-in person is a user; the roles below are on top.
-- Groups come from the company directory (the ID token's groups claim), refreshed at each sign-in.

CREATE TABLE groups (
    id TEXT PRIMARY KEY,
    -- The directory's id for the group (Entra ID: its object id), as the groups claim names it.
    external_id TEXT NOT NULL UNIQUE,
    -- A name a platform admin gave it: the claim carries ids only.
    name TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE group_members (
    group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, user_id)
);
CREATE INDEX group_members_user ON group_members (user_id);

CREATE TABLE role_grants (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('platform_admin', 'team_admin', 'auditor')),
    -- The group a team admin looks after; none for the other roles.
    group_id TEXT REFERENCES groups(id) ON DELETE CASCADE,
    -- Where the grant comes from: 'directory' (the ID token's roles claim) and 'config'
    -- (COMPANION_PLATFORM_ADMINS) are renewed at each sign-in; 'admin' was granted in Companion.
    source TEXT NOT NULL CHECK (source IN ('directory', 'config', 'admin')),
    granted_by TEXT NOT NULL DEFAULT '',
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((role = 'team_admin') = (group_id IS NOT NULL))
);
CREATE UNIQUE INDEX role_grants_once ON role_grants (user_id, role, COALESCE(group_id, ''), source);

-- The local person of an install without sign-in has every role (it is their PC).
INSERT INTO role_grants (user_id, role, source, granted_by) VALUES
    ('local', 'platform_admin', 'config', 'install'),
    ('local', 'auditor', 'config', 'install');
