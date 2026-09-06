ALTER TABLE spaces
    ADD COLUMN IF NOT EXISTS voice_provider TEXT,
    ADD COLUMN IF NOT EXISTS voice_model_id TEXT,
    ADD COLUMN IF NOT EXISTS voice_id TEXT,
    ADD COLUMN IF NOT EXISTS voice_api_key TEXT;
