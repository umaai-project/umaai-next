//! 决策上下文：训练员决策的对外记录（引擎侧 DTO）
//!
//! 训练员只负责「选」；决策上下文由 [`DecisionSource`] 接缝提供——它把搜索 / 策略的
//! 内部状态翻译成本模块的 DTO，供记录与运气分消费。不提供上下文（`None`）的路径
//! 照常运行，只是记录里没有候选与评分。
//!
//! DTO 归引擎而不归计算层：它是记录 / 呈现关注点，计算层不该背这份形态。

use serde::{Deserialize, Serialize};

/// 一个候选动作
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// 候选的人类可读描述（前端按下标映射动作名）
    pub description: String,
    /// 搜索均分；无评分路径记 `None`
    pub score: Option<f64>,
    /// 该候选的搜索次数；无评分路径记 0
    pub n: u32,
}

/// 一次决策的上下文
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    /// 决策类别（机器可读键，如 `train` / `event` / `race`；取值由剧本侧约定）
    pub kind: String,
    /// 选中候选在 [`Self::candidates`] 中的下标；无候选时记 0
    pub action_index: usize,
    /// 选中候选的描述（无候选路径下也必须有值）
    pub action_desc: String,
    /// 候选列表；无评分路径可为空
    pub candidates: Vec<Candidate>,
    /// 剧本自定义附加字段
    pub extra: Option<serde_json::Value>,
}

impl DecisionRecord {
    /// 只带选中动作的极简记录（无候选、无评分）
    pub fn action_only(kind: impl Into<String>, action_desc: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            action_index: 0,
            action_desc: action_desc.into(),
            candidates: vec![],
            extra: None,
        }
    }
}

/// 决策上下文提供方：引擎与训练员之间的可选接缝
///
/// 实现方通常需要内部可变性（`RefCell` / `Mutex`）：训练员的 `select_action` 取
/// `&self`，而决策上下文要在其中被记录。引擎在**每个阶段边界**查询一次，取走即清空。
pub trait DecisionSource {
    /// 取走自上次取用以来产生的决策上下文；无则 `None`
    fn take_decision(&self) -> Option<DecisionRecord>;
}
