UPDATE taught_skills SET recording = '{}'::jsonb WHERE id IN (
 SELECT id FROM taught_skills WHERE status IN ('saved','failed','draft')
 AND updated_at < now() - make_interval(days => $1) AND recording <> '{}'::jsonb
 ORDER BY updated_at LIMIT $2 FOR UPDATE SKIP LOCKED
)
