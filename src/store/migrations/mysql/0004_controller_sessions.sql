ALTER TABLE controllers
    ADD COLUMN session_id VARCHAR(255) NULL,
    ADD COLUMN connected_replica VARCHAR(255) NULL,
    ADD COLUMN observed_at_ms BIGINT NOT NULL DEFAULT 0;
