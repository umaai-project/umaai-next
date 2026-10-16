//! # umasim（计算层）
//!
//! 带行为的领域类型与全部契约 trait，纯计算，不含任何终端依赖。
//!
//! 当前内容：
//!
//! - **规则基座**：`game/{uma,support_card,inherit}`、`base/{person,action,basic}`、
//!   通用枚举（`TurnStage` / `PersonType` / `FriendState` 系列）。
//! - **契约 trait**：`game/traits`（`Person` / `ActionEnum` / `Game` / `Trainer`
//!   与 [`TrainingValueSource`] 表来源钩子）。
//!
//! 后续阶段：搜索框架（`flat_search` / `searchable` / `result` / `terminal`）、
//! 策略与评估（`trainer/*`、`neural/*`）。
//!
//! 纪律：**不含任何 `println!`、颜色、表格、日志与文件路径硬编码**；
//! 诊断与呈现走上层回调（`umaai` / `umaai-cli`），静态数据经 `umaai-core::data`。
//!
//! 剧本钩子（`ScenarioRules` / `ScenarioLifecycle` 等）暂不落地，
//! 待有具体剧本时按「带默认实现、非破坏扩充」的纪律补充。
//!
//! [`TrainingValueSource`]: crate::game::traits::TrainingValueSource

#![warn(missing_docs)]

#[cfg(test)]
pub mod testutil;

pub mod game;
