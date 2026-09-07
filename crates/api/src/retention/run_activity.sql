DELETE FROM run_activity WHERE id IN (
 SELECT id FROM run_activity WHERE created_at < now() - make_interval(days => $1)
 ORDER BY created_at LIMIT $2 FOR UPDATE SKIP LOCKED
)
