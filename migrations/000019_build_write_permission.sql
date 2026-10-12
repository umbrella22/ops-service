-- ====================================================================
-- 迁移: 000019_build_write_permission
-- 描述: 补充 build:write 权限
--   build_triggers CRUD（P4 Git 自动触发）要求 build:write 权限，
--   原种子目录只有 build:read/execute/output_detail，
--   admin/operator 角色均无法管理触发器。
-- ====================================================================

INSERT INTO permissions (resource, action, description)
VALUES ('build', 'write', 'Manage build triggers and build configuration')
ON CONFLICT DO NOTHING;

-- admin 角色获得全部权限（含新增）
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p WHERE r.name = 'admin'
ON CONFLICT DO NOTHING;

-- operator 角色获得 build:write（可管理触发器）
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE r.name = 'operator'
  AND p.resource = 'build' AND p.action = 'write'
ON CONFLICT DO NOTHING;
