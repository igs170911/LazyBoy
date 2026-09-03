-- Phase 1: turn the original one-thread-per-bot model into durable sessions.
ALTER TABLE threads DROP CONSTRAINT IF EXISTS threads_bot_id_key;
ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS title TEXT NOT NULL DEFAULT 'New session',
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'active',
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS next_message_seq INTEGER NOT NULL DEFAULT 1,
    ADD COLUMN IF NOT EXISTS history_summary TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS history_summary_seq INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS history_compacted_at TIMESTAMPTZ;

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS seq INTEGER,
    ADD COLUMN IF NOT EXISTS client_nonce TEXT,
    ADD COLUMN IF NOT EXISTS blocks JSONB NOT NULL DEFAULT '[]'::jsonb;

WITH numbered AS (
    SELECT id, row_number() OVER (PARTITION BY thread_id ORDER BY created_at, id)::INTEGER AS seq
    FROM messages
)
UPDATE messages SET seq = numbered.seq
FROM numbered
WHERE messages.id = numbered.id AND messages.seq IS NULL;

ALTER TABLE messages ALTER COLUMN seq SET NOT NULL;

UPDATE threads t
SET next_message_seq = COALESCE((SELECT max(m.seq) + 1 FROM messages m WHERE m.thread_id = t.id), 1),
    next_event_seq = GREATEST(t.next_event_seq, COALESCE((SELECT max(e.seq) FROM events e WHERE e.thread_id = t.id), 0)),
    updated_at = GREATEST(t.created_at, COALESCE((SELECT max(m.created_at) FROM messages m WHERE m.thread_id = t.id), t.created_at));

CREATE INDEX IF NOT EXISTS threads_bot_updated_idx ON threads (bot_id, updated_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS messages_thread_seq_key ON messages (thread_id, seq);
CREATE UNIQUE INDEX IF NOT EXISTS messages_thread_client_nonce_key
    ON messages (thread_id, client_nonce) WHERE client_nonce IS NOT NULL;

ALTER TABLE runs
    ADD COLUMN IF NOT EXISTS checkpoint JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS retry_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS max_retries INTEGER NOT NULL DEFAULT 3;

CREATE INDEX IF NOT EXISTS runs_claim_idx
    ON runs (status, lease_expires_at, created_at);
CREATE INDEX IF NOT EXISTS events_thread_created_idx ON events (thread_id, seq);
