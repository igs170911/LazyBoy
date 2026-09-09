-- Group routing: an explicit host, and the audience resolved per message.
-- A room used to wake every member for every message; the audience is now
-- decided once when the message arrives, so it is stored with the message.

ALTER TABLE rooms
    ADD COLUMN IF NOT EXISTS host_bot_id TEXT REFERENCES bots (id) ON DELETE SET NULL;

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS reply_bot_ids TEXT[];

-- Existing rooms keep today's behaviour: the first member is the host.
UPDATE rooms
   SET host_bot_id = (
           SELECT m.bot_id
             FROM room_members m
            WHERE m.room_id = rooms.id
            ORDER BY m.created_at, m.bot_id
            LIMIT 1
       )
 WHERE host_bot_id IS NULL;
