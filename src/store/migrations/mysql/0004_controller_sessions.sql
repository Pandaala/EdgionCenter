ALTER TABLE controllers ADD COLUMN session_id VARCHAR(255) NULL;
ALTER TABLE controllers ADD COLUMN connected_replica VARCHAR(255) NULL;
ALTER TABLE controllers ADD COLUMN observed_at_ms BIGINT NOT NULL DEFAULT 0;

-- Rows left online by a previous process cannot represent a live local stream.
UPDATE controllers SET online = 0, session_id = NULL, connected_replica = NULL;
