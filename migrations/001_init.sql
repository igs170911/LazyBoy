CREATE TABLE users (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE spaces (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT TRUE,
    default_model_provider TEXT NOT NULL DEFAULT 'xai',
    default_model_id TEXT NOT NULL DEFAULT 'grok-4.6',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX spaces_user_id_idx ON spaces (user_id);

CREATE TABLE computers (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL REFERENCES spaces (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    scope TEXT NOT NULL,
    scope_key TEXT NOT NULL UNIQUE,
    home_key TEXT NOT NULL UNIQUE,
    home_revision TEXT NOT NULL DEFAULT 'empty',
    kind TEXT NOT NULL DEFAULT 'docker',
    provider_ref TEXT,
    state TEXT NOT NULL DEFAULT 'stopped',
    control_holder TEXT NOT NULL DEFAULT 'none',
    control_lease_id TEXT,
    control_lease_expires_at TIMESTAMPTZ,
    control_bot_id TEXT,
    control_run_id TEXT,
    control_fence INTEGER NOT NULL DEFAULT 0,
    execution_run_id TEXT,
    execution_bot_id TEXT,
    execution_lease_expires_at TIMESTAMPTZ,
    execution_fence INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX computers_space_scope_idx ON computers (space_id, scope);

CREATE TABLE bots (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL REFERENCES spaces (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    instructions TEXT NOT NULL DEFAULT '',
    computer_id TEXT REFERENCES computers (id) ON DELETE SET NULL,
    model_provider TEXT,
    model_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX bots_space_user_idx ON bots (space_id, user_id);
CREATE INDEX bots_computer_id_idx ON bots (computer_id);

CREATE TABLE computer_execution_leases (
    id TEXT PRIMARY KEY,
    computer_id TEXT NOT NULL REFERENCES computers (id) ON DELETE CASCADE,
    bot_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    fence INTEGER NOT NULL DEFAULT 0,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (computer_id, bot_id)
);

CREATE INDEX computer_execution_leases_run_idx ON computer_execution_leases (run_id);
CREATE INDEX computer_execution_leases_expiry_idx ON computer_execution_leases (computer_id, expires_at);

CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL REFERENCES spaces (id) ON DELETE CASCADE,
    bot_id TEXT UNIQUE REFERENCES bots (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    next_event_seq INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    run_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX messages_thread_idx ON messages (thread_id, created_at);

CREATE TABLE runs (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL REFERENCES spaces (id) ON DELETE CASCADE,
    bot_id TEXT NOT NULL REFERENCES bots (id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    status TEXT NOT NULL,
    trigger TEXT NOT NULL DEFAULT 'message',
    prompt TEXT NOT NULL DEFAULT '',
    lease_owner TEXT,
    lease_fence INTEGER NOT NULL DEFAULT 0,
    lease_expires_at TIMESTAMPTZ,
    error TEXT,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX runs_space_status_idx ON runs (space_id, status, updated_at);
CREATE INDEX runs_bot_status_idx ON runs (bot_id, status);

CREATE TABLE events (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    type TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (thread_id, seq)
);
