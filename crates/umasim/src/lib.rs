//! # umasim（计算层）
//!
//! 带行为的领域类型与全部契约 trait，纯计算，不含任何终端依赖。
//!
//! 当前内容：规则基座（`game/{uma,support_card,inherit}`、`base/*`、通用枚举）、契约 trait
//! （`game/traits`：`Person` / `ActionEnum` / `Game` / `Trainer` / [`TrainingValueSource`]）、
//! 搜索框架（`search/*`：扁平搜索内核 + 剧本适配层 + 终局多维记录）、训练员（`trainer`）。
//!
//! 后续阶段：策略与评估扩展（更多训练员、`neural/*`）。
//!
//! 纪律：不含 `println!`、颜色、表格与路径硬编码；诊断走 `umaai_core::diag` 与 `log` 门面，
//! 呈现只在 `umaai-cli`；剧本钩子待有具体剧本时按「带默认实现、非破坏扩充」补充。
//!
//! [`TrainingValueSource`]: crate::game::traits::TrainingValueSource

#![warn(missing_docs)]

#[cfg(test)]
pub mod testutil;

pub mod game;
pub mod search;
pub mod trainer;
