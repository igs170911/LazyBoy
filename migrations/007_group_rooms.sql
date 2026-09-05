-- Multi-agent group rooms: one conversation many bots can speak in.

CREATE TABLE IF NOT EXISTS rooms (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL REFERENCES spaces (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS rooms_space_user_idx ON rooms (space_id, user_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS room_members (
    room_id TEXT NOT NULL REFERENCES rooms (id) ON DELETE CASCADE,
    bot_id TEXT NOT NULL REFERENCES bots (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (room_id, bot_id)
);

CREATE INDEX IF NOT EXISTS room_members_bot_idx ON room_members (bot_id);

ALTER TABLE threads ADD COLUMN IF NOT EXISTS room_id TEXT REFERENCES rooms (id) ON DELETE CASCADE;
CREATE INDEX IF NOT EXISTS threads_room_idx ON threads (room_id) WHERE room_id IS NOT NULL;

ALTER TABLE messages ADD COLUMN IF NOT EXISTS speaker_bot_id TEXT REFERENCES bots (id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS messages_speaker_idx ON messages (speaker_bot_id) WHERE speaker_bot_id IS NOT NULL;
