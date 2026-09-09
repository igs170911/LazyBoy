-- Deleting a conversation that holds a remembered message used to fail.
--
-- `DELETE FROM threads` cascades in two steps that Postgres runs one after the
-- other: the thread's messages are removed first, and only then is
-- memory_items.session_id set to NULL. That second step fires this guard while
-- source_message_id still points at a message that is already gone, so the
-- guard raised "memory message is outside agent scope" and the delete rolled
-- back — the human saw a conversation that could not be removed.
--
-- The guard now checks only the references that this write actually changes.
-- A referential action that clears one column leaves the others as they were,
-- and those were validated when they were written. An INSERT, or a move to
-- another agent/space/user, still validates everything; pointing the item at a
-- different session re-checks that its run and message belong to that session.
-- The same race exists for runs (also removed with the thread), so clearing
-- session_id never re-checks the run or message.
CREATE OR REPLACE FUNCTION validate_memory_item_scope() RETURNS trigger AS $$
DECLARE
    moved BOOLEAN := TG_OP = 'INSERT'
        OR NEW.bot_id IS DISTINCT FROM OLD.bot_id
        OR NEW.space_id IS DISTINCT FROM OLD.space_id
        OR NEW.user_id IS DISTINCT FROM OLD.user_id;
    resessioned BOOLEAN := TG_OP = 'UPDATE'
        AND NEW.session_id IS NOT NULL
        AND NEW.session_id IS DISTINCT FROM OLD.session_id;
BEGIN
    IF NEW.session_id IS NOT NULL
       AND (moved OR resessioned)
       AND NOT EXISTS (
        SELECT 1 FROM threads t
        WHERE t.id = NEW.session_id AND (
            (t.room_id IS NULL AND t.bot_id = NEW.bot_id)
            OR EXISTS (
                SELECT 1 FROM rooms room JOIN room_members member ON member.room_id=room.id
                WHERE room.id=t.room_id AND member.bot_id=NEW.bot_id
                  AND room.space_id=NEW.space_id AND room.user_id=NEW.user_id
            )
          )
          AND t.space_id = NEW.space_id AND t.user_id = NEW.user_id
    ) THEN
        RAISE EXCEPTION 'memory session is outside agent scope';
    END IF;
    IF NEW.source_run_id IS NOT NULL
       AND (moved OR resessioned OR NEW.source_run_id IS DISTINCT FROM OLD.source_run_id)
       AND NOT EXISTS (
        SELECT 1 FROM runs r
        WHERE r.id = NEW.source_run_id AND r.bot_id = NEW.bot_id
          AND r.space_id = NEW.space_id AND r.user_id = NEW.user_id
          AND (NEW.session_id IS NULL OR r.thread_id = NEW.session_id)
    ) THEN
        RAISE EXCEPTION 'memory run is outside agent scope';
    END IF;
    IF NEW.source_message_id IS NOT NULL
       AND (moved OR resessioned OR NEW.source_message_id IS DISTINCT FROM OLD.source_message_id)
       AND NOT EXISTS (
        SELECT 1 FROM messages m
        JOIN threads t ON t.id = m.thread_id
        WHERE m.id = NEW.source_message_id AND (
            (t.room_id IS NULL AND t.bot_id = NEW.bot_id)
            OR EXISTS (
                SELECT 1 FROM rooms room JOIN room_members member ON member.room_id=room.id
                WHERE room.id=t.room_id AND member.bot_id=NEW.bot_id
                  AND room.space_id=NEW.space_id AND room.user_id=NEW.user_id
            )
          )
          AND t.space_id = NEW.space_id AND t.user_id = NEW.user_id
          AND (NEW.session_id IS NULL OR t.id = NEW.session_id)
    ) THEN
        RAISE EXCEPTION 'memory message is outside agent scope';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
