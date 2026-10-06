-- The Companion's schema, moved from SQLite to PostgreSQL (Phase 1, task 1).
-- Same tables and columns; three changes of kind:
-- * every table whose rows were read in insertion order gets `seq`, an
--   increasing number (SQLite used its hidden rowid for this);
-- * 0/1 numbers become booleans, sizes and counts 64-bit integers;
-- * timestamps stay as RFC 3339 text for now, as the code passes them.
-- Owners on records (user ids) arrive with users in task 5.

CREATE TABLE conversations (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    model_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    mode TEXT NOT NULL DEFAULT 'chat',
    workspace TEXT NOT NULL DEFAULT '',
    reasoning_default BOOLEAN NOT NULL DEFAULT FALSE,
    search_default BOOLEAN NOT NULL DEFAULT FALSE,
    last_model TEXT NOT NULL DEFAULT '',
    priority TEXT NOT NULL DEFAULT 'normal',
    related_to TEXT NOT NULL DEFAULT ''
);
CREATE INDEX conversations_by_workspace ON conversations (workspace);

CREATE TABLE messages (
    seq BIGINT GENERATED ALWAYS AS IDENTITY UNIQUE,
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX messages_by_conversation ON messages (conversation_id, seq);

CREATE TABLE tool_executions (
    seq BIGINT GENERATED ALWAYS AS IDENTITY UNIQUE,
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    tool TEXT NOT NULL,
    args TEXT NOT NULL,
    result TEXT NOT NULL,
    approved BOOLEAN NOT NULL DEFAULT FALSE,
    approval TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE INDEX tool_executions_by_conversation ON tool_executions (conversation_id, seq);

CREATE TABLE artifacts (
    seq BIGINT GENERATED ALWAYS AS IDENTITY UNIQUE,
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    filename TEXT NOT NULL,
    path TEXT NOT NULL,
    mime TEXT NOT NULL,
    size_bytes BIGINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE INDEX artifacts_by_conversation ON artifacts (conversation_id, seq);

CREATE TABLE attachments (
    seq BIGINT GENERATED ALWAYS AS IDENTITY UNIQUE,
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    filename TEXT NOT NULL,
    mime TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    text_excerpt TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'text',
    status TEXT NOT NULL DEFAULT 'ready',
    created_at TEXT NOT NULL
);
CREATE INDEX attachments_by_conversation ON attachments (conversation_id, seq);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE workspaces (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    build_system TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE TABLE search_runs (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    query TEXT NOT NULL,
    provider TEXT NOT NULL,
    result_count BIGINT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX search_runs_by_conversation ON search_runs (conversation_id);

CREATE TABLE memory_entries (
    seq BIGINT GENERATED ALWAYS AS IDENTITY UNIQUE,
    id TEXT PRIMARY KEY,
    scope_id TEXT NOT NULL DEFAULT '',
    scope TEXT NOT NULL DEFAULT 'conversation',
    content TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    last_used TEXT NOT NULL
);
CREATE INDEX memory_entries_by_scope ON memory_entries (scope, scope_id);

CREATE TABLE knowledge_chunks (
    seq BIGINT GENERATED ALWAYS AS IDENTITY UNIQUE,
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL DEFAULT '',
    path TEXT NOT NULL DEFAULT '',
    chunk_idx BIGINT NOT NULL DEFAULT 0,
    text TEXT NOT NULL
);
CREATE INDEX knowledge_chunks_by_workspace ON knowledge_chunks (workspace_id, path, chunk_idx);

CREATE TABLE generation_metrics (
    seq BIGINT GENERATED ALWAYS AS IDENTITY UNIQUE,
    message_id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL DEFAULT '',
    model_id TEXT NOT NULL DEFAULT '',
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    generated_tokens BIGINT NOT NULL DEFAULT 0,
    gen_ms BIGINT NOT NULL DEFAULT 0,
    ttft_ms BIGINT NOT NULL DEFAULT 0,
    gen_tps DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT '',
    timing_json TEXT
);
CREATE INDEX generation_metrics_by_conversation ON generation_metrics (conversation_id);

CREATE TABLE context_summaries (
    conversation_id TEXT PRIMARY KEY,
    source_ids TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE message_activities (
    seq BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    message_id TEXT NOT NULL,
    event_json TEXT NOT NULL
);
CREATE INDEX message_activities_by_message ON message_activities (message_id, seq);

CREATE TABLE session_task_context (
    conversation_id TEXT PRIMARY KEY,
    workspace TEXT NOT NULL,
    task TEXT NOT NULL,
    run_id TEXT NOT NULL,
    status TEXT NOT NULL
);

CREATE TABLE runtime_calibrations (
    id TEXT PRIMARY KEY,
    model_id TEXT NOT NULL,
    model_key TEXT NOT NULL,
    created_at TEXT NOT NULL,
    calibration_json TEXT NOT NULL
);
CREATE INDEX runtime_calibrations_by_model ON runtime_calibrations (model_id, model_key, created_at);

CREATE TABLE model_requests (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL DEFAULT '',
    owner_id TEXT NOT NULL DEFAULT '',
    seq BIGINT NOT NULL DEFAULT 0,
    kind TEXT NOT NULL,
    request_json TEXT NOT NULL,
    raw_output TEXT NOT NULL DEFAULT '',
    finish_reason TEXT,
    outcome TEXT NOT NULL,
    failure TEXT,
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    cached_tokens BIGINT NOT NULL DEFAULT 0,
    generated_tokens BIGINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE INDEX model_requests_by_conversation ON model_requests (conversation_id, owner_id, seq);
CREATE INDEX model_requests_by_time ON model_requests (created_at);

CREATE TABLE model_template_caps (
    model_key TEXT PRIMARY KEY,
    caps_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE runtime_fit_decisions (
    fit_key TEXT PRIMARY KEY,
    decision_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
