CREATE TABLE IF NOT EXISTS cloud_capability_snapshots (
    provider_account_id VARBINARY(512) NOT NULL,
    scope_key VARBINARY(2048) NOT NULL,
    scope_json LONGTEXT CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
    provider_account_generation BIGINT NOT NULL,
    credential_revision VARBINARY(512) NULL,
    discovery_epoch BIGINT NOT NULL,
    discovery_token VARBINARY(512) NOT NULL,
    snapshot_json LONGTEXT CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NULL,
    snapshot_provider_account_generation BIGINT NULL,
    snapshot_credential_revision VARBINARY(512) NULL,
    snapshot_discovery_epoch BIGINT NULL,
    snapshot_discovery_token VARBINARY(512) NULL,
    snapshot_write_token VARBINARY(64) NULL,
    PRIMARY KEY (provider_account_id, scope_key),
    CHECK (provider_account_generation > 0),
    CHECK (discovery_epoch > 0)
) ENGINE=InnoDB;
