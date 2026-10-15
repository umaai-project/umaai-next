//! 统一错误类型与结果别名。

use thiserror::Error;

/// 核心层统一错误
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum UmaError {
    /// 输入非法：调用方传入的数据不满足契约
    #[error("输入非法: {0}")]
    InvalidInput(String),
    /// 状态非法：当前状态不允许该操作
    #[error("状态非法: {0}")]
    InvalidState(String),
    /// 内部错误：不应发生的逻辑分支
    #[error("内部错误: {0}")]
    Internal(String),
}

/// 核心层统一结果别名
pub type UmaResult<T> = std::result::Result<T, UmaError>;
