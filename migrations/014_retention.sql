-- Age scans use small batches rather than locking or scanning the full database.
CREATE INDEX IF NOT EXISTS events_retention_idx ON events(created_at);
CREATE INDEX IF NOT EXISTS runs_retention_idx ON runs((COALESCE(completed_at,updated_at)))
    WHERE status IN ('completed','failed','cancelled');
CREATE INDEX IF NOT EXISTS recording_retention_idx ON taught_skills(updated_at)
    WHERE status IN ('saved','failed');
CREATE INDEX IF NOT EXISTS memory_revisions_age_idx ON memory_revisions(created_at);
CREATE INDEX IF NOT EXISTS memory_deleted_age_idx ON memory_items(deleted_at) WHERE deleted_at IS NOT NULL;
-- Encourage normal background vacuum to reuse space after regular small deletes.
ALTER TABLE events SET (autovacuum_vacuum_scale_factor=0.05, autovacuum_vacuum_threshold=1000);
ALTER TABLE runs SET (autovacuum_vacuum_scale_factor=0.05, autovacuum_vacuum_threshold=1000);
ALTER TABLE memory_revisions SET (autovacuum_vacuum_scale_factor=0.05, autovacuum_vacuum_threshold=1000);
