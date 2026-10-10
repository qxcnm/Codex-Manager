-- GPT-6.1 Sol is added through the versioned built-in model fixture.
INSERT INTO model_catalog_v2_meta(key,value)
VALUES('model_catalog_revision10_source','2026-09-30-openai-gpt-6.1-sol')
ON CONFLICT(key) DO UPDATE SET value=excluded.value;
