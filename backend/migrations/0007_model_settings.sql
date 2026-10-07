-- Each model's own settings (Phase 1 task 9, per the owner): its context, sampling, performance and
-- hardware, laid over the company's defaults when it loads. Only what differs from the company is kept.

CREATE TABLE model_settings (
    model_id TEXT PRIMARY KEY,
    settings JSONB NOT NULL DEFAULT '{}',
    updated_by TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
