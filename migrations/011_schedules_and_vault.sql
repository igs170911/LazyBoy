CREATE TABLE IF NOT EXISTS vault_accounts (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    bot_id TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE,
    site TEXT NOT NULL,
    host TEXT NOT NULL DEFAULT '',
    username TEXT NOT NULL,
    password_ciphertext TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS vault_accounts_bot_idx ON vault_accounts (bot_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS schedules (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    bot_id TEXT NOT NULL REFERENCES bots(id) ON DELETE CASCADE,
    thread_id TEXT REFERENCES threads(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    cron TEXT NOT NULL,
    timezone TEXT NOT NULL DEFAULT 'Asia/Taipei',
    instructions TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_run_at TIMESTAMPTZ,
    next_run_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS schedules_due_idx ON schedules (enabled, next_run_at)
    WHERE enabled AND next_run_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS schedules_bot_idx ON schedules (bot_id, enabled, updated_at DESC);
