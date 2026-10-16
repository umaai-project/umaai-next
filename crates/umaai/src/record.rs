//! 记录装配：把决策上下文与运气分装配成可下发的记录
//!
//! 引擎只做装配，**不落盘**：CSV / 归档 / 趋势图等由上层实现（见 open_items #15）。
//! 记录的数据形态是 [`DecisionRecord`]。
//!
//! [`LuckScorer`] 是运气分的调用侧状态机：把「本回合基线」交给
//! [`LuckScoreTracker`]，再把快照与逐候选偏差挂回记录。是否挂载由调用方决定——
//! **不调用即不挂载、也不推进基线**，这正是旧实现在地区选择一类回合刻意整段跳过的语义。

use crate::decision::{DecisionRecord, LuckAttach};
use crate::luck::{LuckScoreSnapshot, LuckScoreTracker};

/// 运气分装配器（持一条运气分跟踪器与显示分换算加成）
#[derive(Debug, Clone)]
pub struct LuckScorer {
    tracker: LuckScoreTracker,
    bonus: i32,
}

impl LuckScorer {
    /// 建立装配器；`bonus` 为显示分换算的每回合加成（旧仓沿用 70）
    pub fn new(bonus: i32) -> Self {
        Self {
            tracker: LuckScoreTracker::new(),
            bonus,
        }
    }

    /// 显示分换算加成
    pub fn bonus(&self) -> i32 {
        self.bonus
    }

    /// 当前切局键（尚未记录过任何一局时为 `None`）
    pub fn epoch_key(&self) -> Option<u64> {
        self.tracker.epoch_key()
    }

    /// 最近一次快照
    pub fn snapshot(&self) -> LuckScoreSnapshot {
        self.tracker.snapshot()
    }

    /// 给一条决策挂运气分，返回本回合运气；首次 / 切局后为 `None`
    ///
    /// 无带评分候选的决策**不挂载、也不推进基线**：基线是「上一回合搜索期望分」，
    /// 让无评分决策参与会凭空造出一段差值。
    pub fn attach(&mut self, rec: &mut DecisionRecord, epoch_key: u64, turn: i32, max_turn: i32) -> Option<f64> {
        if !rec.has_scores() {
            return None;
        }
        let baseline = rec.baseline_score();
        let delta = self
            .tracker
            .on_new_turn(epoch_key, baseline, turn, max_turn, self.bonus);
        let action_luck = rec.candidates.iter().map(|c| c.score.map(|s| s - baseline)).collect();
        rec.luck = Some(LuckAttach {
            snapshot: self.tracker.snapshot(),
            action_luck,
        });
        delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::Candidate;

    fn record(scores: &[(f64, u32)]) -> DecisionRecord {
        DecisionRecord {
            kind: "train".to_string(),
            action_index: 0,
            action_desc: "选中".to_string(),
            candidates: scores
                .iter()
                .map(|(s, n)| Candidate {
                    description: format!("{s}"),
                    score: Some(*s),
                    n: *n,
                })
                .collect(),
            luck: None,
            extra: None,
        }
    }

    /// 层1：挂载后快照与逐候选偏差就位
    #[test]
    fn test_attach_fills_luck() {
        let mut scorer = LuckScorer::new(1);
        let mut rec = record(&[(100.0, 2), (200.0, 2)]);
        // 首回合：基线 = (100×2+200×2)/4 = 150；turn=0, max=78, bonus=1
        assert_eq!(scorer.attach(&mut rec, 42, 0, 78), None);
        let luck = rec.luck.expect("应挂载");
        assert_eq!(luck.snapshot.initial_terminal_baseline, 228.0);
        assert_eq!(luck.snapshot.current_terminal_baseline, 228.0);
        assert_eq!(luck.snapshot.total_luck_score, 0.0);
        assert_eq!(luck.action_luck, vec![Some(-50.0), Some(50.0)]);
        assert_eq!(scorer.epoch_key(), Some(42));
    }

    /// 层1：多回合推进基线并累计总运气
    #[test]
    fn test_attach_accumulates() {
        let mut scorer = LuckScorer::new(1);
        let mut a = record(&[(50000.0, 1)]);
        let mut b = record(&[(50150.0, 1)]);
        scorer.attach(&mut a, 42, 0, 78);
        // turn=1：current = 50150+77 = 50227；delta = 50227−50078 = 149
        assert_eq!(scorer.attach(&mut b, 42, 1, 78), Some(149.0));
        assert_eq!(b.luck.expect("应挂载").snapshot.total_luck_score, 149.0);
        assert_eq!(scorer.snapshot().last_turn_delta, Some(149.0));
    }

    /// 层1：无评分决策不挂载且不推进基线
    #[test]
    fn test_skip_unscored() {
        let mut scorer = LuckScorer::new(1);
        let mut a = record(&[(50000.0, 1)]);
        scorer.attach(&mut a, 42, 0, 78);
        let before = scorer.snapshot();

        let mut skipped = DecisionRecord::action_only("event", "无评分");
        assert_eq!(scorer.attach(&mut skipped, 42, 1, 78), None);
        assert!(skipped.luck.is_none(), "无评分决策不挂载");
        assert_eq!(scorer.snapshot(), before, "基线不推进");

        // 下一回合仍从上一有效回合起算：delta = (50150+76) − 50078 = 148
        let mut b = record(&[(50150.0, 1)]);
        assert_eq!(scorer.attach(&mut b, 42, 2, 78), Some(148.0));
    }

    /// 层1：切局重置
    #[test]
    fn test_epoch_change() {
        let mut scorer = LuckScorer::new(1);
        let mut a = record(&[(50000.0, 1)]);
        let mut b = record(&[(48000.0, 1)]);
        scorer.attach(&mut a, 42, 0, 78);
        assert_eq!(scorer.attach(&mut b, 99, 1, 78), None, "切局首回合为空");
        assert_eq!(b.luck.expect("仍应挂载").snapshot.total_luck_score, 0.0);
        assert_eq!(scorer.epoch_key(), Some(99));
    }
}
