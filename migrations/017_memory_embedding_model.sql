-- Same dimensions do not mean the same embedding space. Keep legacy vectors
-- labeled so reads cannot compare them to the new multilingual model. Content
-- and revisions stay intact while the background indexer replaces old vectors.
ALTER TABLE memory_items ADD COLUMN IF NOT EXISTS embedding_model TEXT;
UPDATE memory_items SET embedding_model='all-MiniLM-L6-v2'
WHERE embedding IS NOT NULL AND embedding_model IS NULL;
