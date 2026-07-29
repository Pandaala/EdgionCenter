-- The single all-method `proxy:access` key was split into `proxy:read`
-- (GET/HEAD) and `proxy:write` (every other method) so a database role can
-- express a read-only proxy grant. Rewrite stored grants in place: a role that
-- could read and write through the proxy keeps both halves, and no role is left
-- holding a key the catalog no longer knows. Leaving the stale key behind is not
-- merely a dead grant -- the roles page re-submits held non-catalog keys on
-- save, and `validate_keys` rejects unknown keys with 400, which would make the
-- role uneditable through the dashboard.
INSERT IGNORE INTO role_permissions (role_id, permission_key)
SELECT role_id, 'proxy:read' FROM role_permissions WHERE permission_key = 'proxy:access';

INSERT IGNORE INTO role_permissions (role_id, permission_key)
SELECT role_id, 'proxy:write' FROM role_permissions WHERE permission_key = 'proxy:access';

DELETE FROM role_permissions WHERE permission_key = 'proxy:access';
