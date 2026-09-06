DELETE FROM memory_revisions WHERE (memory_id,revision) IN (
 SELECT r.memory_id,r.revision FROM memory_revisions r JOIN memory_items m ON m.id=r.memory_id
 WHERE r.revision < m.revision AND (
   r.created_at < now() - make_interval(days => $1)
   OR r.revision <= m.revision - 10
 ) ORDER BY r.created_at LIMIT $2 FOR UPDATE OF r SKIP LOCKED
)
