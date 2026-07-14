ALTER TABLE controllers ADD COLUMN session_id TEXT;
ALTER TABLE controllers ADD COLUMN connected_replica TEXT;
ALTER TABLE controllers ADD COLUMN observed_at_ms INTEGER NOT NULL DEFAULT 0;

-- Rows left online by a previous process cannot represent a live local stream.
UPDATE controllers SET online = 0, session_id = NULL, connected_replica = NULL, observed_at_ms = 0;
