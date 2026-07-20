CREATE TABLE IF NOT EXISTS cloud_provider_accounts (
    account_id VARBINARY(512) NOT NULL,
    generation BIGINT NOT NULL,
    desired_json LONGTEXT CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
    PRIMARY KEY (account_id),
    CHECK (generation > 0),
    CHECK (OCTET_LENGTH(desired_json) <= 65536)
) ENGINE=InnoDB;
