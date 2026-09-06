DELETE FROM events WHERE id IN (
 SELECT id FROM events WHERE created_at < now() - make_interval(days => $1)
 ORDER BY created_at LIMIT $2 FOR UPDATE SKIP LOCKED
)
