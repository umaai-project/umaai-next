//! 游戏常量（constants.json）的纯 schema/DTO
//!
//! 用户可调项（比赛等级表 / PT 偏好倍率 / MCTS 每回合加成）不在本表，归配置层。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Array6;

/// 游戏常量表
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameConstants {
    /// 五维属性上限基值（1200 不减半）
    pub five_status_limit_base: [i32; 5],
    /// 训练名字
    pub train_names: Vec<String>,
    /// 干劲名字
    pub motivation_names: Vec<String>,
    /// 训练失败体力阈值：`[训练位][等级-1]`
    pub training_vital_threshold: Vec<Vec<f32>>,
    /// 团队卡 buff 解除概率
    pub group_buff_end_prob: Vec<f64>,
    /// 每点 pt 对应分数
    pub pt_score_rate: f32,
    /// 每级 Hint 对应的 pt
    pub hint_pt_rate: f32,
    /// 五维 → 终局评分（原始属性索引，1200 以上未减半）
    pub five_status_final_score: Vec<i32>,
    /// 评分档次分界
    pub rank_scores: Vec<i32>,
    /// 评价名字
    pub rank_names: Vec<String>,
    /// 事件出现概率
    pub event_probs: BTreeMap<String, f64>,
    /// 不能出现随机事件的回合
    pub no_event_turns: Vec<i32>,
    /// 基础 Hint 率
    pub base_hint_rate: f64,
    /// 休息结果分布（+30 / +50 / +70 的百分比）
    pub rest_probs: Vec<i32>,
    /// 红点属性事件数值
    pub hint_event_value: Vec<Array6>,
    /// 每张卡最大提供 Hint 等级
    pub max_hint_per_card: i32,
    /// PT 特化时超过 1200 的属性压缩系数
    pub five_status_favor_rate: Vec<f32>,
}

impl GameConstants {
    /// 训练轴名（`train_names` 取名，越界 / 缺失退化为 `#i`）
    ///
    /// 轴名的唯一出口：纯文本 `Display`（`ActionValue` 增减摘要）与终端呈现
    /// （回合行 / 五维行）共用，不在代码里写第二份。
    #[must_use]
    pub fn train_name(&self, i: usize) -> String {
        self.train_names.get(i).cloned().unwrap_or_else(|| format!("#{i}"))
    }

    /// 查五维属性对应的终局评分，越界饱和到表末（空表返回 0）
    pub fn status_final_score(&self, status: i32) -> i32 {
        match self.five_status_final_score.last() {
            None => 0,
            Some(&last) => {
                let idx = (status.max(0) as usize).min(self.five_status_final_score.len() - 1);
                if idx == self.five_status_final_score.len() - 1 {
                    last
                } else {
                    self.five_status_final_score[idx]
                }
            }
        }
    }

    /// 查评分对应评价名
    ///
    /// `rank_scores[i]` 为升到 `rank_names[i]` 的分数线（`rank_names[0]` 为基准档）。
    /// 两端饱和：未达首档取最低档、超过末档取最高档；空表回退 `"US9"`（不 panic）。
    pub fn get_rank_name(&self, score: i32) -> &str {
        let s = score.max(0);
        let fallback = "US9";
        match self.rank_scores.iter().position(|&x| s < x) {
            // 未达任何分数线：最低档
            Some(0) => self.rank_names.first().map(String::as_str).unwrap_or(fallback),
            // 已达 rank_scores[i]：落在其前一档
            Some(i) => self.rank_names.get(i - 1).map(String::as_str).unwrap_or(fallback),
            // 超过全部分界线：最高档
            None => self.rank_names.last().map(String::as_str).unwrap_or(fallback),
        }
    }

    /// 随机事件分布：支援卡 / 马娘 / 掉干劲 / 不发生
    pub fn get_event_distribution(&self) -> [f64; 4] {
        let p = |k: &str| self.event_probs.get(k).copied().unwrap_or(0.0);
        let (card, uma, drop) = (p("card_event"), p("uma_event"), p("drop_motivation"));
        [card, uma, drop, 1.0 - card - uma - drop]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consts() -> GameConstants {
        GameConstants {
            five_status_limit_base: [3100, 2400, 2200, 2200, 2400],
            train_names: vec!["速".into(), "耐".into(), "力".into(), "根".into(), "智".into()],
            motivation_names: vec!["绝好调".into()],
            training_vital_threshold: vec![],
            group_buff_end_prob: vec![0.0],
            pt_score_rate: 2.0,
            hint_pt_rate: 6.5,
            five_status_final_score: vec![0, 10, 20],
            rank_scores: vec![0, 100, 200],
            rank_names: vec!["G".into(), "F".into(), "E".into()],
            event_probs: BTreeMap::from([
                ("card_event".to_string(), 0.35),
                ("uma_event".to_string(), 0.25),
                ("drop_motivation".to_string(), 0.1),
            ]),
            no_event_turns: vec![],
            base_hint_rate: 7.5,
            rest_probs: vec![18, 57, 25],
            hint_event_value: vec![],
            max_hint_per_card: 60,
            five_status_favor_rate: vec![],
        }
    }

    /// 层1：评分查表两端饱和
    #[test]
    fn test_status_final_score_saturates() {
        let c = consts();
        assert_eq!(c.status_final_score(-5), 0);
        assert_eq!(c.status_final_score(1), 10);
        assert_eq!(c.status_final_score(2), 20);
        assert_eq!(c.status_final_score(9999), 20, "越界应饱和到表末");
    }

    /// 层1：评价名按分数线取档，两端饱和
    #[test]
    fn test_rank_name() {
        let c = consts();
        assert_eq!(c.get_rank_name(-1), "G", "低于首档取最低档");
        assert_eq!(c.get_rank_name(0), "G");
        assert_eq!(c.get_rank_name(99), "G");
        assert_eq!(c.get_rank_name(100), "F");
        assert_eq!(c.get_rank_name(199), "F");
        assert_eq!(c.get_rank_name(200), "E", "达到末档分数线取最高档");
        assert_eq!(c.get_rank_name(99999), "E", "超过全部分界取最高档");
    }

    /// 层1：事件分布第四项为剩余概率
    #[test]
    fn test_event_distribution() {
        let d = consts().get_event_distribution();
        assert!((d[0] - 0.35).abs() < 1e-12);
        assert!((d[1] - 0.25).abs() < 1e-12);
        assert!((d[2] - 0.1).abs() < 1e-12);
        assert!((d[3] - 0.3).abs() < 1e-12);
    }
}
