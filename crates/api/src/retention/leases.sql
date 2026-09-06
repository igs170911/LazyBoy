DELETE FROM computer_execution_leases WHERE id IN (
 SELECT l.id FROM computer_execution_leases l WHERE l.expires_at < now() - make_interval(days => $1)
 AND NOT EXISTS (SELECT 1 FROM runs r WHERE r.id=l.run_id AND r.status NOT IN ('completed','failed','cancelled'))
 ORDER BY l.expires_at LIMIT $2 FOR UPDATE SKIP LOCKED
)
