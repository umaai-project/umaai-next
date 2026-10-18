//! # umaai-tools（探针 / 基准 / 基线，`publish = false`）
//!
//! 引擎开发**必需**的开发工具，与 `umaai-cli` 同属主线样例面：不进发布产物，pub 面
//! 不承诺稳定，下游（Web / Android / 剧本仓）不得依赖。dev 重依赖（criterion 等）若
//! 将来确需引入，只允许出现在本 crate。
//!
//! - [`batch`]：固定种子批量跑局（逐局终局汇总 → 评分分布与吞吐）。
//! - [`probe`]：固定根搜索成本探针（首个训练决策点，量预算与单点耗时）。
//! - [`baseline`]：基线 JSON 的模型 / 跑法 / 比对 / 读写；判定壳在 `umaai-tests`。
//! - [`report`]：输出原语（机读单行 + 人读面板与表格），复用 `umaai-cli` 的呈现件。
//! - [`cli`]：入口逻辑（参数解析 + 子命令分发），bin 只收参数与给退出码。
//!
//! 纪律：工具件只**测量与记录**，不改决策语义；跑局装配链与 `umaai-cli` 共用同一对
//! 装配函数，不另立一套选择器口径。CSV / 归档 / 趋势图等落盘格式按真实消费者冻结，
//! 当前只出机读单行与 stdout 摘要。
#![warn(missing_docs)]

pub mod baseline;
pub mod batch;
pub mod cli;
pub mod probe;
pub mod report;
mod setup;
