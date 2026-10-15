//! # umaai-core（核心层）
//!
//! 契约层。职责：
//!
//! - **钩子 trait**：`ScenarioRules`、`ScenarioLifecycle`、`TrainingValueSource`、`FlatSearchGame`、
//!   `ActionIndex` —— 全部提供默认空实现。
//! - **策略 trait**：`Trainer`、`Evaluator`、`RolloutBackend`、`Policy`。
//! - **输出 trait**：`Frontend`（info / error / decision / record 回调）、`DecisionSink`。
//! - **基础类型**：`Status`、`Turn`、`ScenarioId`、`PersonId`、`ActionId`、`Score`、`EngineError` 等。
//! - **RNG 三流**：可复现核心资产，逐字移植。
//!
//! 纪律：
//!
//! - **零重依赖**：只允许 `serde`（derive）/ `thiserror` 这类轻量库；不得引入终端、网络、计算重依赖。
//! - 跨边界类型 `#[derive(Serialize, Deserialize, Debug, Clone)]`；公开枚举 `#[non_exhaustive]`。
//! - trait 新方法**必须带默认实现**（非破坏性演进）。
//!
//! 设计见 `docs/plan/miko_next_plan.md`。
#![warn(missing_docs)]
