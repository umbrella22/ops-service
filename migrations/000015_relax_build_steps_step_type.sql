-- ====================================================================
-- 迁移: 000015_relax_build_steps_step_type
-- 描述: build_steps.step_type 放宽为可空
--   webhook 路径（build_log/build_status 回调）懒创建步骤行时不携带
--   step_type（类型只存在于 build_jobs.build_parameters 中），
--   NOT NULL 约束导致真实数据库上步骤日志/状态写入必然失败。
-- ====================================================================

ALTER TABLE build_steps
    ALTER COLUMN step_type DROP NOT NULL;

COMMENT ON COLUMN build_steps.step_type IS 'Step type (clone/compile/test/...); NULL for lazily-created rows from runner callbacks';
