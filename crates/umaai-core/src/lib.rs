//! # umaai-core（核心层）
//!
//! 与游戏无关的通用机制层：零重依赖，可被引擎与各剧本仓共同复用。
//!
//! 当前内容：
//!
//! - [`rng`]：受控随机原语（无状态流 + 类型隔离三流 + 种子派生）。
//! - [`error`]：统一错误 [`UmaError`] 与结果别名 [`UmaResult`]。
//! - [`array`]：固定长度属性数组别名（[`Array5`] / [`Array6`]）与逐元素工具
//!   （[`AttributeArray`] / [`split_status`]）。
//! - [`stats`]：通用统计原语（均值 / 方差 / 标准差 / 中位数 / 分位）。
//! - [`crn`]：CRN rollout 种子表 [`RolloutSeeds`]。
//! - [`flags`]：局面状态标识 [`UmaFlags`] 与生病位集 [`Ill`]。
//! - [`race_mask`]：比赛回合位段 [`RaceMask`]（bit i ↔ 回合 11+i）。
//! - [`gamedata`]：静态数据（umaDB / cardDB / events / constants）的纯 schema/DTO。
//! - [`data`]：静态数据载入 / 清单与语义校验 / 全局便捷层 [`Data`]。
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

pub mod array;
pub mod crn;
pub mod data;
pub mod error;
pub mod flags;
pub mod gamedata;
pub mod race_mask;
pub mod rng;
pub mod stats;
#[doc(hidden)]
pub mod testing;

pub use array::{Array5, Array6, AttributeArray, split_status};
pub use crn::RolloutSeeds;
pub use data::Data;
pub use error::{UmaError, UmaResult};
pub use flags::{Ill, UmaFlags};
pub use race_mask::RaceMask;
