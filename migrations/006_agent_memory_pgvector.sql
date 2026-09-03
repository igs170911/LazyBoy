-- Phase 2: durable, per-agent memory with local pgvector embeddings.
CREATE EXTENSION IF NOT EXISTS vector;

ALTER TABLE bots
    ADD COLUMN IF NOT EXISTS memory_enabled BOOLEAN NOT NULL DEFAULT TRUE;

-- Enables a scope-preserving foreign key without changing the existing primary key.
CREATE UNIQUE INDEX IF NOT EXISTS bots_id_space_user_key
    ON bots (id, space_id, user_id);

CREATE TABLE IF NOT EXISTS memory_items (
    id UUID PRIMARY KEY,
    space_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    bot_id TEXT NOT NULL,
    session_id TEXT,
    source_run_id TEXT,
    source_message_id TEXT,
    content TEXT NOT NULL CHECK (length(btrim(content)) > 0),
    importance REAL NOT NULL DEFAULT 0.5 CHECK (importance >= 0 AND importance <= 1),
    embedding vector(384),
    search_document TSVECTOR GENERATED ALWAYS AS (to_tsvector('simple', content)) STORED,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    CONSTRAINT memory_items_bot_scope_fk
        FOREIGN KEY (bot_id, space_id, user_id)
        REFERENCES bots (id, space_id, user_id) ON DELETE CASCADE,
    CONSTRAINT memory_items_session_fk
        FOREIGN KEY (session_id) REFERENCES threads (id) ON DELETE SET NULL,
    CONSTRAINT memory_items_source_run_fk
        FOREIGN KEY (source_run_id) REFERENCES runs (id) ON DELETE SET NULL,
    CONSTRAINT memory_items_source_message_fk
        FOREIGN KEY (source_message_id) REFERENCES messages (id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS memory_revisions (
    memory_id UUID NOT NULL REFERENCES memory_items (id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision > 0),
    space_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    bot_id TEXT NOT NULL,
    content TEXT NOT NULL,
    importance REAL NOT NULL CHECK (importance >= 0 AND importance <= 1),
    session_id TEXT,
    source_run_id TEXT,
    source_message_id TEXT,
    action TEXT NOT NULL CHECK (action IN ('create', 'update', 'delete', 'restore')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (memory_id, revision),
    CONSTRAINT memory_revisions_bot_scope_fk
        FOREIGN KEY (bot_id, space_id, user_id)
        REFERENCES bots (id, space_id, user_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS memory_items_scope_active_idx
    ON memory_items (space_id, user_id, bot_id, updated_at DESC)
    WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS memory_items_session_idx
    ON memory_items (session_id) WHERE session_id IS NOT NULL AND deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS memory_items_source_run_idx
    ON memory_items (source_run_id) WHERE source_run_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS memory_items_source_message_idx
    ON memory_items (source_message_id) WHERE source_message_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS memory_items_search_idx
    ON memory_items USING GIN (search_document);
CREATE INDEX IF NOT EXISTS memory_items_embedding_hnsw_idx
    ON memory_items USING hnsw (embedding vector_cosine_ops)
    WHERE embedding IS NOT NULL AND deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS memory_revisions_scope_idx
    ON memory_revisions (space_id, user_id, bot_id, memory_id, revision DESC);

-- Optional source references must belong to the same actor, bot, and (when supplied)
-- session. This prevents accidental cross-agent linkage even from future callers.
CREATE OR REPLACE FUNCTION validate_memory_item_scope() RETURNS trigger AS $$
BEGIN
    IF NEW.session_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM threads t
        WHERE t.id = NEW.session_id AND t.bot_id = NEW.bot_id
          AND t.space_id = NEW.space_id AND t.user_id = NEW.user_id
    ) THEN
        RAISE EXCEPTION 'memory session is outside agent scope';
    END IF;
    IF NEW.source_run_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM runs r
        WHERE r.id = NEW.source_run_id AND r.bot_id = NEW.bot_id
          AND r.space_id = NEW.space_id AND r.user_id = NEW.user_id
          AND (NEW.session_id IS NULL OR r.thread_id = NEW.session_id)
    ) THEN
        RAISE EXCEPTION 'memory run is outside agent scope';
    END IF;
    IF NEW.source_message_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM messages m
        JOIN threads t ON t.id = m.thread_id
        WHERE m.id = NEW.source_message_id AND t.bot_id = NEW.bot_id
          AND t.space_id = NEW.space_id AND t.user_id = NEW.user_id
          AND (NEW.session_id IS NULL OR t.id = NEW.session_id)
    ) THEN
        RAISE EXCEPTION 'memory message is outside agent scope';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS memory_items_scope_guard ON memory_items;
CREATE TRIGGER memory_items_scope_guard
    BEFORE INSERT OR UPDATE OF space_id, user_id, bot_id, session_id, source_run_id, source_message_id
    ON memory_items FOR EACH ROW EXECUTE FUNCTION validate_memory_item_scope();
