-- Per-run activity trail for the chat's live monitor bubble. Diagnostics only:
-- never user-authored content, and cheap enough to write once per model turn.
CREATE TABLE IF NOT EXISTS run_activity (
    id BIGSERIAL PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS run_activity_run_idx ON run_activity (run_id, id);
CREATE INDEX IF NOT EXISTS run_activity_retention_idx ON run_activity (created_at);
