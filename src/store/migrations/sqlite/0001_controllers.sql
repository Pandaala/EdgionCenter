CREATE TABLE IF NOT EXISTS controllers (
    controller_id TEXT PRIMARY KEY,
    cluster TEXT NOT NULL DEFAULT '',
    env TEXT NOT NULL DEFAULT '[]',
    tag TEXT NOT NULL DEFAULT '[]',
    online INTEGER NOT NULL DEFAULT 0,
    last_seen_at INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT 0
);

-- Drop legacy tables (idempotent, safe on both old and new databases).
DROP TABLE IF EXISTS region_route_cache;
DROP TABLE IF EXISTS cluster_plugin_metadata_cache;
DROP TABLE IF EXISTS service_plugin_metadata_cache;
