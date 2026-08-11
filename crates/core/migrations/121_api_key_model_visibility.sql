ALTER TABLE api_key_policies
  ADD COLUMN model_visibility TEXT NOT NULL DEFAULT 'selectable'
  CHECK (model_visibility IN ('selectable', 'managed'));