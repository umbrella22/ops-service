-- ====================================================================
-- 迁移: 000016_build_cancel_dispatch
-- 描述: 支持构建取消下发到 Runner
--   1. build_jobs.runner_name：记录派发目标（可观测性 + 定向排障）。
--      取消消息以 build.cancel.<job_id> 广播，不依赖此列，
--      此列仅用于审计与状态展示。
-- ====================================================================

ALTER TABLE build_jobs
    ADD COLUMN IF NOT EXISTS runner_name VARCHAR(255);

COMMENT ON COLUMN build_jobs.runner_name IS 'Runner that accepted/dispatched this build (observability only; cancel is broadcast)';
