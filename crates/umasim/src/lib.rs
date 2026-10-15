//! # umasim（计算层）
//!
//! 纯计算，不含任何终端依赖。职责：
//!
//! - **规则基座**：`game/{uma,support_card,inherit,simulator}`、`base/{person,action}`、
//!   通用枚举（`TurnStage` / `PersonType` / `FriendState`）。
//! - **搜索框架**：`flat_search` / `searchable` / `result` / `seeds` / `terminal`。
//! - **策略与评估**：`trainer/*`（去剧本特化）、`neural/{evaluator,value_output}`。
//! - **通用输出**：`output/diagnostic` + `DecisionSink` 的实现。
//! - **数据加载**：`data` 模块（单一入口；目录由宿主注入，见方案 v2 的「数据与配置」节）。
//!
//! 纪律：**不含任何 `println!`、颜色、进度条、文件路径硬编码**（数据目录由上层注入）。
#![warn(missing_docs)]
