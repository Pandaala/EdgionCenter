CREATE TABLE IF NOT EXISTS audit_log (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    ts BIGINT NOT NULL,
    actor VARCHAR(255) NOT NULL DEFAULT '<unknown>',
    provider VARCHAR(255) NOT NULL DEFAULT '',
    method VARCHAR(255) NOT NULL,
    path TEXT NOT NULL,
    target_controller VARCHAR(255) NULL,
    status INT NOT NULL,
    source_ip VARCHAR(255) NULL,
    request_id VARCHAR(255) NULL,
    detail TEXT NULL,
    INDEX idx_audit_log_ts (ts),
    INDEX idx_audit_log_actor (actor)
);
