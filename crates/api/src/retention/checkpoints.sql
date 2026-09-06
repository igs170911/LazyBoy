UPDATE runs SET checkpoint = '{}'::jsonb WHERE id IN (
 SELECT id FROM runs WHERE status IN ('completed','failed','cancelled')
 AND COALESCE(completed_at,updated_at) < now() - make_interval(days => $1)
 AND checkpoint <> '{}'::jsonb
 ORDER BY COALESCE(completed_at,updated_at) LIMIT $2 FOR UPDATE SKIP LOCKED
)
