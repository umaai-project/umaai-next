//! # umaai-tests（集成 / 端到端测试，`publish = false`）
//!
//! 测试支撑与集成测试的家：
//!
//! - `Checker`：打印 + 聚合，失败返回 `Err`（**不用 `assert`**，保持 `cargo test` 有失败信号）。
//! - 共享夹具：替代旧库 `#[path = "..."]` 跨目录引入的写法。
//! - baseline 加载 / 比对：`tests/baseline.rs` + 基线数据文件（自动判定）。
//!
//! 测试分层与约定见 `docs/plan/miko_next_plan.md` 的「测试策略」节。
#![warn(missing_docs)]
