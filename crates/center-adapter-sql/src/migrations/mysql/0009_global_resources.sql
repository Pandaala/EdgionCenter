CREATE TABLE IF NOT EXISTS global_resources (
    global_resource_id VARBINARY(512) NOT NULL,
    contract_version BIGINT NOT NULL,
    generation BIGINT NOT NULL,
    resource_json LONGTEXT CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
    PRIMARY KEY (global_resource_id),
    CHECK (contract_version > 0),
    CHECK (generation > 0),
    CHECK (OCTET_LENGTH(resource_json) BETWEEN 1 AND 1048576)
) ENGINE=InnoDB;
