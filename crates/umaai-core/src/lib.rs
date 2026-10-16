//! # umaai-core（核心层）
//!
//! 与游戏无关的通用机制层：零重依赖，可被引擎与各剧本仓共同复用。
//!
//! 模块：`rng`（受控随机原语）/ `error`（统一错误）/ `diag`（诊断输出与开关）/
//! `array`（属性数组与逐元素工具）/ `stats`（切片式统计原语 + 增量矩统计）/ `crn`（CRN 种子表）/
//! `flags`（局面状态标识）/ `race_mask`（比赛回合位段）/ `gamedata`（静态数据 schema）/
//! `data`（载入 / 校验 / 全局便捷层）。
//!
//! 纪律：只允许轻量、跨平台、通用的依赖（不得引入终端、网络、计算重依赖）；跨边界类型
//! `#[derive(Serialize, Deserialize, Debug, Clone)]`；公开枚举 `#[non_exhaustive]`；
//! trait 新方法必须带默认实现。
//!
//! 游戏契约（`Game` / 剧本钩子 / 搜索 / 输出）不在本层，归 `umasim`。

#![warn(missing_docs)]

pub mod array;
pub mod crn;
pub mod data;
pub mod diag;
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
pub use stats::Moments;
