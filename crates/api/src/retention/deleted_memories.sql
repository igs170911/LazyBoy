DELETE FROM memory_items WHERE id IN (
 SELECT id FROM memory_items WHERE deleted_at < now() - make_interval(days => $1)
 ORDER BY deleted_at LIMIT $2 FOR UPDATE SKIP LOCKED
)
