-- 补齐 assets_hosts 主机密钥验证策略字段
-- 模型（models/asset.rs）与 SSH 执行器已使用这两个字段，
-- 但 000007 漏加对应列，导致 SELECT * FROM assets_hosts 解码失败。

ALTER TABLE assets_hosts
    ADD COLUMN IF NOT EXISTS host_key_verification VARCHAR(20),
    ADD COLUMN IF NOT EXISTS known_hosts JSONB;

-- 验证策略取值约束（NULL 表示沿用全局默认）
ALTER TABLE assets_hosts
    DROP CONSTRAINT IF EXISTS assets_hosts_host_key_verification_check;

ALTER TABLE assets_hosts
    ADD CONSTRAINT assets_hosts_host_key_verification_check
    CHECK (host_key_verification IS NULL OR host_key_verification IN ('strict', 'accept', 'disabled'));
