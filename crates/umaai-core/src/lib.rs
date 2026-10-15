//! # umaai-core（核心层）
//!
//! 与游戏无关的通用机制层：零重依赖，可被引擎与各剧本仓共同复用。
//!
//! 当前内容：
//!
//! - [`rng`]：受控随机原语（无状态流 + 类型隔离三流 + 种子派生）。
//! - [`error`]：统一错误 [`UmaError`] 与结果别名 [`UmaResult`]。
//!
//! 纪律：
//!
//! - **零重依赖**：只允许 `serde` / `serde_json` / `thiserror` / `rand` 这类通用库；
//!   不得引入终端、网络、计算重依赖。
//! - 跨边界类型 `#[derive(Serialize, Deserialize, Debug, Clone)]`；公开枚举 `#[non_exhaustive]`。
//! - trait 新方法**必须带默认实现**（非破坏性演进）。
//!
//! 游戏契约（`Game` / 剧本钩子 / 搜索 / 输出）不在本层，归 `umasim`。

#![warn(missing_docs)]

pub mod error;
pub mod rng;

pub use error::{UmaError, UmaResult};
