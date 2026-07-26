CREATE TABLE IF NOT EXISTS global_resources (
    global_resource_id BLOB PRIMARY KEY NOT NULL
        CHECK (typeof(global_resource_id) = 'blob'
            AND length(global_resource_id) BETWEEN 1 AND 512),
    contract_version INTEGER NOT NULL
        CHECK (typeof(contract_version) = 'integer'
            AND contract_version > 0),
    generation INTEGER NOT NULL
        CHECK (typeof(generation) = 'integer'
            AND generation > 0),
    resource_json TEXT NOT NULL
        CHECK (typeof(resource_json) = 'text'
            AND length(CAST(resource_json AS BLOB)) BETWEEN 1 AND 1048576)
) WITHOUT ROWID;
