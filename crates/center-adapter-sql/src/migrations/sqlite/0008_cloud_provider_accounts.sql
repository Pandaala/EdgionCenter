CREATE TABLE IF NOT EXISTS cloud_provider_accounts (
    account_id BLOB PRIMARY KEY NOT NULL
        CHECK (typeof(account_id) = 'blob'
            AND length(account_id) BETWEEN 1 AND 512),
    generation INTEGER NOT NULL
        CHECK (typeof(generation) = 'integer'
            AND generation > 0),
    desired_json TEXT NOT NULL
        CHECK (typeof(desired_json) = 'text'
            AND length(CAST(desired_json AS BLOB)) <= 65536)
) WITHOUT ROWID;
