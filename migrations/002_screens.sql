ALTER TABLE computers
    ADD COLUMN IF NOT EXISTS browser_profile_mode TEXT NOT NULL DEFAULT 'per-bot';

CREATE TABLE IF NOT EXISTS computer_screens (
    id TEXT PRIMARY KEY,
    computer_id TEXT NOT NULL REFERENCES computers (id) ON DELETE CASCADE,
    bot_id TEXT NOT NULL,
    slot INTEGER NOT NULL,
    display TEXT NOT NULL,
    view_port INTEGER NOT NULL,
    profile_mode TEXT NOT NULL DEFAULT 'per-bot',
    profile_path TEXT NOT NULL,
    control_holder TEXT NOT NULL DEFAULT 'none',
    control_lease_id TEXT,
    control_lease_expires_at TIMESTAMPTZ,
    execution_run_id TEXT,
    execution_lease_expires_at TIMESTAMPTZ,
    execution_fence INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (computer_id, bot_id),
    UNIQUE (computer_id, slot)
);

CREATE INDEX IF NOT EXISTS computer_screens_bot_idx ON computer_screens (bot_id);
CREATE INDEX IF NOT EXISTS computer_screens_run_idx ON computer_screens (execution_run_id);

CREATE TABLE IF NOT EXISTS computer_profile_locks (
    computer_id TEXT NOT NULL REFERENCES computers (id) ON DELETE CASCADE,
    profile_key TEXT NOT NULL,
    bot_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (computer_id, profile_key)
);
