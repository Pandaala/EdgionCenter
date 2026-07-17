CREATE TABLE IF NOT EXISTS cloud_operations (
    queue_order INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    idempotency_key TEXT NOT NULL UNIQUE,
    request_json TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    resource_kind TEXT NOT NULL,
    action TEXT NOT NULL,
    desired_generation INTEGER NOT NULL,
    requested_by TEXT NOT NULL,
    phase TEXT NOT NULL,
    cancel_requested INTEGER NOT NULL DEFAULT 0,
    created_at_unix_ms INTEGER NOT NULL,
    updated_at_unix_ms INTEGER NOT NULL,
    deadline_unix_ms INTEGER,
    next_attempt_at_unix_ms INTEGER,
    steps_json TEXT NOT NULL,
    lease_holder TEXT,
    lease_token TEXT UNIQUE,
    fencing_epoch INTEGER NOT NULL DEFAULT 0,
    lease_valid_until_unix_ms INTEGER
);

CREATE INDEX IF NOT EXISTS idx_cloud_operations_ready
    ON cloud_operations (phase, next_attempt_at_unix_ms, lease_valid_until_unix_ms, created_at_unix_ms);
CREATE INDEX IF NOT EXISTS idx_cloud_operations_resource_queue
    ON cloud_operations (resource_kind, resource_id, queue_order);
