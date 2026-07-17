CREATE TABLE IF NOT EXISTS cloud_operations (
    queue_order BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    id VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL UNIQUE,
    idempotency_key VARCHAR(512) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL UNIQUE,
    request_json LONGTEXT CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
    resource_id VARCHAR(512) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
    resource_kind VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
    action VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
    desired_generation BIGINT NOT NULL,
    requested_by TEXT NOT NULL,
    phase VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
    cancel_requested TINYINT NOT NULL DEFAULT 0,
    created_at_unix_ms BIGINT NOT NULL,
    updated_at_unix_ms BIGINT NOT NULL,
    deadline_unix_ms BIGINT NULL,
    next_attempt_at_unix_ms BIGINT NULL,
    steps_json LONGTEXT CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
    lease_holder VARCHAR(512) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NULL,
    lease_token VARCHAR(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NULL UNIQUE,
    fencing_epoch BIGINT NOT NULL DEFAULT 0,
    lease_valid_until_unix_ms BIGINT NULL,
    INDEX idx_cloud_operations_ready
        (phase, next_attempt_at_unix_ms, lease_valid_until_unix_ms, created_at_unix_ms),
    INDEX idx_cloud_operations_resource_queue
        (resource_kind, resource_id, queue_order)
) ENGINE=InnoDB;
