-- Phase 1, task 8: audit records. One row per model request, tool call, web search, approval,
-- admin or settings change, and sign-in. Rows are only ever added: the server cannot change or
-- remove one (the fingerprint chain, retention and the viewer follow in Phase 7).

CREATE TABLE audit_records (
    seq BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Who: a users row, 'system' for the server's own work, '' for a failed sign-in.
    user_id TEXT NOT NULL,
    -- How they were acting: local, session, api key, agent, system.
    via TEXT NOT NULL,
    -- From which device: the connection's address and the browser's or app's name.
    address TEXT NOT NULL DEFAULT '',
    device TEXT NOT NULL DEFAULT '',
    -- model_request, tool_call, web_search, approval, admin_change, settings_change,
    -- sign_in, sign_out, api_key.
    action TEXT NOT NULL,
    -- What it was done to: a conversation, a run, a route.
    target TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    generated_tokens BIGINT NOT NULL DEFAULT 0,
    -- How it was allowed: the permission mode, an approval, a grant, a role.
    allowed_by TEXT NOT NULL DEFAULT '',
    outcome TEXT NOT NULL DEFAULT '',
    -- The rest, per action (tool and arguments, query, request body), masked and shortened.
    detail JSONB NOT NULL DEFAULT '{}'
);
CREATE INDEX audit_records_by_user ON audit_records (user_id, seq);
CREATE INDEX audit_records_by_action ON audit_records (action, seq);

CREATE FUNCTION audit_records_only_grow() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'audit records cannot be changed or removed';
END
$$;
CREATE TRIGGER audit_records_no_change BEFORE UPDATE OR DELETE ON audit_records
    FOR EACH ROW EXECUTE FUNCTION audit_records_only_grow();
CREATE TRIGGER audit_records_no_truncate BEFORE TRUNCATE ON audit_records
    FOR EACH STATEMENT EXECUTE FUNCTION audit_records_only_grow();
