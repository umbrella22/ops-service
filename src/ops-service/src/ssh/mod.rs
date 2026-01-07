//! SSH执行模块
//! P2 阶段：SSH连接管理和命令执行

pub mod executor;

pub use executor::{ExecutionResult, SSHAuth, SSHClient, SSHConfig};
