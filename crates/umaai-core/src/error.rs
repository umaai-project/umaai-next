//! 统一错误类型与结果别名。

use std::path::PathBuf;

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
    /// 读取数据文件失败
    #[error("读取失败 {}: {source}", path.display())]
    Io {
        /// 数据文件路径
        path: PathBuf,
        /// 底层 IO 错误
        source: std::io::Error,
    },
    /// 解析数据文件失败（JSON）
    #[error("解析失败 {}: {source}", path.display())]
    Parse {
        /// 数据文件路径
        path: PathBuf,
        /// 底层解析错误
        source: serde_json::Error,
    },
    /// 数据校验失败
    #[error("校验失败 {target}: {detail}")]
    Validation {
        /// 定位串，形如「文件 / 条目 / 字段」
        target: String,
        /// 具体原因
        detail: String,
    },
}

/// 核心层统一结果别名
pub type UmaResult<T> = std::result::Result<T, UmaError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：IO / 解析 / 校验错误的文案包含关键定位信息
    #[test]
    fn test_error_display() {
        let io = UmaError::Io {
            path: PathBuf::from("gamedata/umaDB.json"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
        };
        let msg = io.to_string();
        assert!(msg.contains("gamedata/umaDB.json"), "IO 错误应含路径: {msg}");
        assert!(msg.contains("not found"), "IO 错误应含底层原因: {msg}");

        let parse = UmaError::Parse {
            path: PathBuf::from("gamedata/events.json"),
            source: serde_json::from_str::<i32>("x").unwrap_err(),
        };
        assert!(parse.to_string().contains("events.json"), "解析错误应含路径");

        let v = UmaError::Validation {
            target: "constants.json/event_probs/card_event".to_string(),
            detail: "缺失".to_string(),
        };
        assert!(v.to_string().contains("event_probs"), "校验错误应含定位串");
    }

    /// 层1：`source` 字段被 thiserror 识别为错误源
    #[test]
    fn test_error_source_chain() {
        use std::error::Error;

        let e = UmaError::Parse {
            path: PathBuf::from("x.json"),
            source: serde_json::from_str::<i32>("x").unwrap_err(),
        };
        assert!(e.source().is_some(), "Parse 应暴露底层 serde_json 错误");
    }
}
