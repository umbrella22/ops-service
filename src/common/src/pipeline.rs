//! Pipeline-as-code：仓库内 `.ops.yml` 管道定义
//!
//! 构建作业可以不通过 API 传 steps，而是让 Runner 在 clone 之后读取
//! 仓库中的管道文件（默认 `.ops.yml`）解析出步骤并执行——与 Drone/
//! GitHub Actions 的"管道随仓库走"模式一致。
//!
//! 示例：
//! ```yaml
//! version: 1
//! env:
//!   CARGO_TERM_COLOR: never
//! steps:
//!   - id: build
//!     name: Build
//!     step_type: script
//!     script: |
//!       cargo build --release
//!     timeout_secs: 600
//!   - id: test
//!     name: Test
//!     step_type: command
//!     command: cargo test
//! ```

use serde::Deserialize;
use std::collections::HashMap;

/// 管道文件默认路径
pub const DEFAULT_PIPELINE_FILE: &str = ".ops.yml";

/// 仓库内管道定义文件
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PipelineFile {
    /// 格式版本（当前仅 1）
    #[serde(default)]
    pub version: Option<u32>,

    /// 步骤级默认环境变量（任务 env_vars 可覆盖）
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// 步骤列表（至少一步）
    #[serde(default)]
    pub steps: Vec<PipelineStep>,
}

/// 管道步骤（字段带默认值，比消息协议的 BuildStep 更宽松）
#[derive(Debug, Clone, Deserialize)]
pub struct PipelineStep {
    /// 步骤 ID（唯一；仅允许字母数字与 . _ -）
    pub id: String,

    /// 步骤名称（缺省用 id）
    pub name: Option<String>,

    /// 步骤类型（缺省 command）
    #[serde(default = "default_step_type")]
    pub step_type: String,

    #[serde(default)]
    pub command: Option<String>,

    #[serde(default)]
    pub script: Option<String>,

    #[serde(default)]
    pub working_dir: Option<String>,

    #[serde(default)]
    pub timeout_secs: Option<u64>,

    #[serde(default)]
    pub continue_on_failure: bool,

    #[serde(default)]
    pub produces_artifact: bool,

    #[serde(default)]
    pub docker_image: Option<String>,
}

fn default_step_type() -> String {
    "command".to_string()
}

impl PipelineFile {
    /// 从 YAML 文本解析
    pub fn parse(yaml: &str) -> Result<Self, String> {
        let file: PipelineFile = serde_yaml::from_str(yaml)
            .map_err(|e| format!("invalid pipeline file: {e}"))?;
        file.validate()?;
        Ok(file)
    }

    /// 结构校验（400 语义；与 API 侧 validate_build_request 对齐）
    pub fn validate(&self) -> Result<(), String> {
        if self.steps.is_empty() {
            return Err("pipeline must define at least one step".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        for step in &self.steps {
            if step.id.is_empty() || step.id.len() > 255 {
                return Err("step id must be 1-255 characters".to_string());
            }
            if !step
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            {
                return Err(format!(
                    "step id '{}' may only contain letters, digits, '.', '_' and '-'",
                    step.id
                ));
            }
            if !seen.insert(step.id.clone()) {
                return Err(format!("duplicate step id '{}'", step.id));
            }
            if step.command.is_none() && step.script.is_none() {
                return Err(format!("step '{}' must have command or script", step.id));
            }
            if let Some(ts) = step.timeout_secs
                && !(1..=86400).contains(&ts)
            {
                return Err(format!(
                    "step '{}' timeout_secs must be between 1 and 86400",
                    step.id
                ));
            }
        }
        Ok(())
    }

    /// 转换为消息协议的 BuildStep 列表
    pub fn to_build_steps(&self) -> Vec<crate::messages::BuildStep> {
        use crate::messages::{BuildStep, StepType};
        self.steps
            .iter()
            .map(|s| BuildStep {
                id: s.id.clone(),
                name: s.name.clone().unwrap_or_else(|| s.id.clone()),
                step_type: match s.step_type.to_lowercase().as_str() {
                    "command" => StepType::Command,
                    "script" => StepType::Script,
                    "install" => StepType::Install,
                    "build" => StepType::Build,
                    "test" => StepType::Test,
                    "package" => StepType::Package,
                    "publish" => StepType::Publish,
                    other => StepType::Custom(other.to_string()),
                },
                command: s.command.clone(),
                script: s.script.clone(),
                working_dir: s.working_dir.clone(),
                timeout_secs: s.timeout_secs,
                continue_on_failure: s.continue_on_failure,
                produces_artifact: s.produces_artifact,
                docker_image: s.docker_image.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
version: 1
env:
  FOO: bar
steps:
  - id: build
    name: Build
    step_type: script
    script: |
      echo building
    timeout_secs: 60
  - id: test
    command: echo testing
    continue_on_failure: true
"#;

    #[test]
    fn test_parse_sample() {
        let file = PipelineFile::parse(SAMPLE).unwrap();
        assert_eq!(file.steps.len(), 2);
        assert_eq!(file.env.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(file.steps[0].id, "build");
        assert_eq!(file.steps[0].timeout_secs, Some(60));
        assert!(file.steps[1].continue_on_failure);
    }

    #[test]
    fn test_parse_rejects_empty_steps() {
        let err = PipelineFile::parse("version: 1\nsteps: []\n").unwrap_err();
        assert!(err.contains("at least one step"));
    }

    #[test]
    fn test_parse_rejects_bad_step_id() {
        let err = PipelineFile::parse("steps:\n  - id: '../evil'\n    command: x\n").unwrap_err();
        assert!(err.contains("letters, digits"));
    }

    #[test]
    fn test_parse_rejects_duplicate_ids() {
        let err = PipelineFile::parse(
            "steps:\n  - id: a\n    command: x\n  - id: a\n    command: y\n",
        )
        .unwrap_err();
        assert!(err.contains("duplicate step id 'a'"));
    }

    #[test]
    fn test_to_build_steps() {
        let file = PipelineFile::parse(SAMPLE).unwrap();
        let steps = file.to_build_steps();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].name, "Build");
        assert!(steps[1].name == "test"); // 缺省用 id
    }
}
