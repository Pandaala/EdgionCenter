CREATE TABLE IF NOT EXISTS controllers (
    controller_id VARCHAR(255) PRIMARY KEY,
    cluster VARCHAR(255) NOT NULL DEFAULT '',
    env TEXT NOT NULL,
    tag TEXT NOT NULL,
    online TINYINT NOT NULL DEFAULT 0,
    last_seen_at BIGINT NOT NULL DEFAULT 0,
    created_at BIGINT NOT NULL DEFAULT 0
);
