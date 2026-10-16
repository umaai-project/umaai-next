//! 决策上下文：训练员决策的对外记录（引擎侧 DTO）
//!
//! 训练员只负责「选」；决策上下文由 [`DecisionSource`] 接缝提供——它把搜索 / 策略的
//! 内部状态翻译成本模块的 DTO，供记录与运气分消费。不提供上下文（`None`）的路径
//! 照常运行，只是记录里没有候选与评分。
//!
//! DTO 归引擎而不归计算层：它是记录 / 呈现关注点，计算层不该背这份形态。

use serde::{Deserialize, Serialize};

use crate::luck::LuckScoreSnapshot;

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

/// 运气分挂载：本回合快照 + 逐候选相对本回合基线的偏差
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LuckAttach {
    /// 本回合运气分快照
    pub snapshot: LuckScoreSnapshot,
    /// 每候选的 `score − 本回合基线`；与 [`DecisionRecord::candidates`] 同长同序，
    /// 未评分候选记 `None`
    pub action_luck: Vec<Option<f64>>,
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
    /// 运气分挂载；未挂（无评分路径或调用方跳过）时为 `None`
    pub luck: Option<LuckAttach>,
    /// 剧本自定义附加字段
    pub extra: Option<serde_json::Value>,
}

impl DecisionRecord {
    /// 只带选中动作的极简记录（无候选、无评分、无运气分）
    pub fn action_only(kind: impl Into<String>, action_desc: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            action_index: 0,
            action_desc: action_desc.into(),
            candidates: vec![],
            luck: None,
            extra: None,
        }
    }

    /// 是否至少有一个候选带搜索评分（决定能否算运气分）
    pub fn has_scores(&self) -> bool {
        self.candidates.iter().any(|c| c.score.is_some())
    }

    /// 本回合基线：带评分候选按搜索次数加权（`Σ score×n / Σ n`）
    ///
    /// 全部搜索次数为 0 时退化为按候选等权；无带评分候选时为 0。
    /// 只统计带评分的候选，未评分的候选不参与加权（其 `n` 不稀释基线）。
    pub fn baseline_score(&self) -> f64 {
        let scored = self.candidates.iter().filter_map(|c| c.score.map(|s| (s, c.n)));
        let (sum_s, sum_sn, count, total_n) = scored.fold((0.0, 0.0, 0usize, 0u32), |(ss, ssn, c, tn), (s, n)| {
            (ss + s, ssn + s * n as f64, c + 1, tn + n)
        });
        if count == 0 {
            return 0.0;
        }
        if total_n == 0 {
            sum_s / count as f64
        } else {
            sum_sn / total_n as f64
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

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(score: Option<f64>, n: u32) -> Candidate {
        Candidate {
            description: format!("{score:?}/{n}"),
            score,
            n,
        }
    }

    /// 层1：基线按搜索次数加权
    #[test]
    fn test_baseline_weighted() {
        let rec = DecisionRecord {
            kind: "train".to_string(),
            action_index: 0,
            action_desc: "a".to_string(),
            // (100×3 + 200×1) / 4 = 125
            candidates: vec![candidate(Some(100.0), 3), candidate(Some(200.0), 1)],
            luck: None,
            extra: None,
        };
        assert_eq!(rec.baseline_score(), 125.0);
        assert!(rec.has_scores());
    }

    /// 层1：搜索次数全为 0 → 按候选等权
    #[test]
    fn test_baseline_equal_weight() {
        let rec = DecisionRecord {
            kind: "train".to_string(),
            action_index: 0,
            action_desc: "a".to_string(),
            candidates: vec![candidate(Some(100.0), 0), candidate(Some(200.0), 0)],
            luck: None,
            extra: None,
        };
        assert_eq!(rec.baseline_score(), 150.0);
    }

    /// 层1：未评分候选不参与加权、也不稀释基线
    #[test]
    fn test_baseline_skips_unscored() {
        let rec = DecisionRecord {
            kind: "train".to_string(),
            action_index: 0,
            action_desc: "a".to_string(),
            candidates: vec![candidate(Some(100.0), 2), candidate(None, 8)],
            luck: None,
            extra: None,
        };
        assert_eq!(rec.baseline_score(), 100.0);
        assert!(rec.has_scores());
    }

    /// 层1：无带评分候选 → 0 且判定为「无评分」
    #[test]
    fn test_baseline_no_scores() {
        let rec = DecisionRecord::action_only("train", "a");
        assert_eq!(rec.baseline_score(), 0.0);
        assert!(!rec.has_scores());
    }
}
