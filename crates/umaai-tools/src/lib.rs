//! # umaai-tools（探针 / 基准，`publish = false`）
//!
//! 收拢引擎开发**必需**的轻量工具：探针 bin、配对基准、以及「策略 × 世界」接口
//! （为将来的 workbench 预留，见方案 v2 的「算法探索 workbench」节）。
//!
//! 不进入发布产物；dev 重依赖（criterion / ndarray / pprof 等）只允许出现在本 crate。
#![warn(missing_docs)]
