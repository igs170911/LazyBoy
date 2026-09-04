ALTER TABLE spaces
    ADD COLUMN IF NOT EXISTS default_model_base_url TEXT,
    ADD COLUMN IF NOT EXISTS default_model_api_key TEXT;
