-- ====================================================================
-- 迁移: 000014_fix_schema_drift
-- 描述: 修复 schema 与代码漂移
--   1. build_artifacts 缺少 updated_at 列（handlers/artifact.rs 会 UPDATE 它）
--   2. build_steps (job_id, step_id) 唯一索引缺失（webhook 写入的行
--      build_job_id/step_order 均为 NULL，导致两条旧约束全部失效）
--   3. 移除 assets_hosts 上的数据库层审计触发器：应用层已写审计
--      （handlers/asset.rs），触发器仅依赖从未被设置的会话变量，
--      造成每次主机变更产生两条 subject 归属错误的重复审计记录
-- ====================================================================

-- 1. build_artifacts.updated_at
ALTER TABLE build_artifacts
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

DROP TRIGGER IF EXISTS update_build_artifacts_updated_at ON build_artifacts;
CREATE TRIGGER update_build_artifacts_updated_at
    BEFORE UPDATE ON build_artifacts
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

-- 2. build_steps 去重后补唯一索引（webhook 路径写入 job_id + step_id）
DELETE FROM build_steps a
USING build_steps b
WHERE a.id < b.id
  AND a.job_id IS NOT NULL
  AND a.job_id = b.job_id
  AND a.step_id IS NOT NULL
  AND a.step_id = b.step_id;

CREATE UNIQUE INDEX IF NOT EXISTS idx_build_steps_job_id_step_id_uniq
    ON build_steps(job_id, step_id)
    WHERE job_id IS NOT NULL AND step_id IS NOT NULL;

-- 3. 应用层已负责 assets_hosts 的审计，移除重复的触发器审计
DROP TRIGGER IF EXISTS audit_assets_hosts_trigger ON assets_hosts;
