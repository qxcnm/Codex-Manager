-- Revision 9 catalog reconciliation is driven by the versioned fixture and
-- Rust storage helpers so fresh and upgraded databases share one code path.
INSERT INTO model_catalog_v2_meta(key,value)
VALUES('model_catalog_revision9_source','2026-09-24-official')
ON CONFLICT(key) DO UPDATE SET value=excluded.value;
