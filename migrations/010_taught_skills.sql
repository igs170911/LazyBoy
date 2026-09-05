-- Skills taught by demonstration: the human drives the bot's desktop while we
-- record semantic events (DOM clicks, typed text, navigations) and keyframes,
-- then a model distils that into an intent-level playbook the bot can follow.
CREATE TABLE taught_skills (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL REFERENCES spaces (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    bot_id TEXT NOT NULL REFERENCES bots (id) ON DELETE CASCADE,
    thread_id TEXT,
    name TEXT NOT NULL DEFAULT '',
    goal TEXT NOT NULL,
    -- recording | drafting | draft | saved | failed
    status TEXT NOT NULL,
    playbook JSONB NOT NULL DEFAULT '{}'::jsonb,
    recording JSONB NOT NULL DEFAULT '{"events":[],"frames":[]}'::jsonb,
    error TEXT,
    started_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    stopped_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX taught_skills_one_recording_idx
    ON taught_skills (bot_id) WHERE status = 'recording';
CREATE INDEX taught_skills_bot_status_idx ON taught_skills (bot_id, status);
