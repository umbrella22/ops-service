-- ====================================================================
-- 迁移: 000017_step_status_timeout_value
-- 描述: step_status 枚举补充 'timeout' 取值
--   Runner 会报告步骤级 Timeout 状态（StepStatus::Timeout），
--   原枚举只有 pending/running/succeeded/failed/skipped，
--   超时步骤的状态更新会因非法枚举值失败。
-- ====================================================================

ALTER TYPE step_status ADD VALUE IF NOT EXISTS 'timeout';
