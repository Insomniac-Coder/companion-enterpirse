-- Phase 1, task 5: an owner on every record. Conversations, projects and saved memories name their
-- person; everything else belongs to a conversation (messages, journals, attachments, artifacts,
-- tool and search records, metrics, summaries, task context, model requests) or to a project
-- (knowledge chunks), and so to that conversation's or project's person.
-- Records from before this (a laptop's) belong to the local person; from now on every new record
-- must name its person (no default).

ALTER TABLE conversations ADD COLUMN user_id TEXT NOT NULL DEFAULT 'local' REFERENCES users(id);
ALTER TABLE conversations ALTER COLUMN user_id DROP DEFAULT;
CREATE INDEX conversations_by_user ON conversations (user_id, created_at);

ALTER TABLE workspaces ADD COLUMN user_id TEXT NOT NULL DEFAULT 'local' REFERENCES users(id);
ALTER TABLE workspaces ALTER COLUMN user_id DROP DEFAULT;
CREATE INDEX workspaces_by_user ON workspaces (user_id, created_at);

ALTER TABLE memory_entries ADD COLUMN user_id TEXT NOT NULL DEFAULT 'local' REFERENCES users(id);
ALTER TABLE memory_entries ALTER COLUMN user_id DROP DEFAULT;
CREATE INDEX memory_entries_by_user ON memory_entries (user_id, scope, scope_id);
