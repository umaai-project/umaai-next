//! # umaai（引擎层）
//!
//! 职责：`Runner` 主循环 · 协议解析（单场景直连）· `Config` · 决策编排 · luck / record
//! （无终端 / IO 重依赖）。把内部状态变化翻译成事件回调给 `Frontend`，**自己绝不打印**。
//!
//! 签名（目标形态）：`Runner::run(cfg, frontend, data) -> Result<Summary>`。
//!
//! 设计见 `docs/plan/miko_next_plan.md`。
#![warn(missing_docs)]
