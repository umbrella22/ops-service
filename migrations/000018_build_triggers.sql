-- ====================================================================
-- 迁移: 000018_build_triggers
-- 描述: Git webhook 自动触发构建
--   外部 Git 服务（GitHub/GitLab）push 时命中触发器自动创建构建作业：
--   按仓库 URL + 分支匹配，签名验证通过后以触发器配置创建管道模式构建
--   （steps 为空 + pipeline_file，由 Runner 从仓库读取 .ops.yml）。
-- ====================================================================

CREATE TABLE IF NOT EXISTS build_triggers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(100) NOT NULL,

    -- webhook 签名密钥（落库前经 crypto_service 加密，API 永不回显）
    secret TEXT NOT NULL,

    -- Git 服务类型（决定验签方式）：github(HMAC-SHA256) | gitlab(Token)
    provider VARCHAR(20) NOT NULL DEFAULT 'github'
        CHECK (provider IN ('github', 'gitlab')),

    -- 匹配条件：仓库精确匹配 + 分支精确匹配（'*' 匹配任意分支）
    repository_url VARCHAR(500) NOT NULL,
    branch VARCHAR(255) NOT NULL DEFAULT '*',

    -- 创建的构建参数
    project_name VARCHAR(255) NOT NULL,
    build_type build_type NOT NULL DEFAULT 'other',
    pipeline_file VARCHAR(255),
    env_vars JSONB NOT NULL DEFAULT '{}',

    enabled BOOLEAN NOT NULL DEFAULT TRUE,

    -- 触发统计
    last_triggered_at TIMESTAMPTZ,
    trigger_count BIGINT NOT NULL DEFAULT 0,

    created_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_build_triggers_repo ON build_triggers(repository_url);
COMMENT ON TABLE build_triggers IS 'Git push 自动触发构建的规则（webhook 签名验证后按仓库+分支匹配）';
