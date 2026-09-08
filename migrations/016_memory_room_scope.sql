-- A group memory belongs to its agent; its source may be their shared room.
-- Private conversations retain the strict agent boundary. Run sources still
-- require the run to belong to the remembering agent.
CREATE OR REPLACE FUNCTION validate_memory_item_scope() RETURNS trigger AS $$
BEGIN
    IF NEW.session_id IS NOT NULL AND NOT EXISTS (
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

