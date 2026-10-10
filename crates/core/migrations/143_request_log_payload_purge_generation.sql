-- Clear purges separate rows written before a clear from rows written after
-- it by the clear generation, not by time.
--
-- A clear marker used to doom rows with rowid <= max_rowid and
-- created_at <= cleared_at. created_at has one-second resolution and SQLite
-- reuses the highest rowid once that row is deleted, so a row written in the
-- same second as the clear could match both conditions and be purged.
--
-- Every write now stores the generation that is current in its own
-- statement, and a clear stores the generation it bumped to. A row is doomed
-- by a clear marker when its generation is lower than the marker's.
--
-- Rows and markers written before this migration keep generation NULL. A
-- legacy marker still uses the old condition, but only for legacy rows.
--
-- (No semicolons in comments: batches are split on every semicolon.)
ALTER TABLE request_log_payloads ADD COLUMN generation INTEGER;
ALTER TABLE request_log_payload_manifests ADD COLUMN generation INTEGER;
ALTER TABLE request_log_upstream_attempts ADD COLUMN generation INTEGER;
ALTER TABLE request_log_response_links ADD COLUMN generation INTEGER;
ALTER TABLE request_log_payload_purges ADD COLUMN generation INTEGER;
