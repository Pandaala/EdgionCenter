CREATE TABLE IF NOT EXISTS cloud_capability_snapshots (
    provider_account_id BLOB NOT NULL,
    scope_key BLOB NOT NULL,
    scope_json TEXT NOT NULL,
    provider_account_generation INTEGER NOT NULL
        CHECK (typeof(provider_account_generation) = 'integer'
            AND provider_account_generation > 0),
    credential_revision BLOB,
    discovery_epoch INTEGER NOT NULL
        CHECK (typeof(discovery_epoch) = 'integer' AND discovery_epoch > 0),
    discovery_token BLOB NOT NULL,
    snapshot_json TEXT,
    snapshot_provider_account_generation INTEGER,
    snapshot_credential_revision BLOB,
    snapshot_discovery_epoch INTEGER,
    snapshot_discovery_token BLOB,
    snapshot_write_token BLOB,
    PRIMARY KEY (provider_account_id, scope_key)
) WITHOUT ROWID;
