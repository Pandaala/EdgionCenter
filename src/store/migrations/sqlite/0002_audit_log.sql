CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts INTEGER NOT NULL,
    actor TEXT NOT NULL DEFAULT '<unknown>',
    provider TEXT NOT NULL DEFAULT '',
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    target_controller TEXT,
    status INTEGER NOT NULL,
    source_ip TEXT,
    request_id TEXT,
    detail TEXT
);

CREATE INDEX IF NOT EXISTS idx_audit_log_ts ON audit_log (ts);
CREATE INDEX IF NOT EXISTS idx_audit_log_actor ON audit_log (actor);
