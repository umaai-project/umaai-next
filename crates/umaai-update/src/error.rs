//! 统一错误类型
//!
//! 更新是「网络 + 文件系统」混合面，错误按**可归因**分类：写盘失败给路径、下载失败给 URL、
//! 清单问题给字段位置、校验失败给期望与实际值——宿主据此决定提示文案与是否阻断。

use std::path::PathBuf;

/// 更新库的错误类型
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum UpdateError {
    /// 文件读写失败（含临时目录与备份）
    #[error("文件操作失败（{path}）: {source}")]
    Io {
        /// 出错的路径
        path: PathBuf,
        /// 底层错误
        #[source]
        source: std::io::Error,
    },
    /// 下载失败（网络、状态码、传输中断）
    #[error("下载失败（{url}）: {detail}")]
    Http {
        /// 请求地址
        url: String,
        /// 失败原因
        detail: String,
    },
    /// 清单不合法（协议版本 / 字段 / 路径 / 哈希格式）
    #[error("清单不合法（{target}）: {detail}")]
    Manifest {
        /// 出错的字段或位置
        target: String,
        /// 说明
        detail: String,
    },
    /// 下载内容与清单登记的哈希不一致
    #[error("文件校验不一致（{path}）: 期望 {expected}，实际 {actual}")]
    Hash {
        /// 相对路径
        path: String,
        /// 清单登记的 sha256
        expected: String,
        /// 实际算出的 sha256
        actual: String,
    },
    /// 数据 schema 版本与程序期望不一致（数据与程序必须同时更新）
    #[error("数据 schema 版本不一致（{channel}）: 程序期望 {expected}，远端为 {found}（需先更新程序）")]
    Schema {
        /// 频道 id
        channel: String,
        /// 程序期望的版本
        expected: u32,
        /// 远端清单声明的版本
        found: u32,
    },
    /// 频道声明了 schema 门控，但远端清单没有声明 schema 版本
    #[error("清单缺少 schema_version（{channel}）: 该频道声明了程序期望版本 {expected}")]
    SchemaMissing {
        /// 频道 id
        channel: String,
        /// 程序期望的版本
        expected: u32,
    },
    /// 安装失败且回滚不完整（目标目录可能处于混合状态，需重跑更新修复）
    #[error("安装失败且回滚不完整: {detail}（原始错误: {origin}）")]
    Rollback {
        /// 回滚失败项说明
        detail: String,
        /// 触发回滚的原始错误
        origin: String,
    },
}

/// 更新库的结果类型
pub type UpdateResult<T> = std::result::Result<T, UpdateError>;
