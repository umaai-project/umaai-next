//! 搜索结果
//!
//! 单轴评分分布统计与搜索输出。排序只用一条评分口径（见 `FlatSearchGame::search_score`），
//! 终局多维观测另由 `search::terminal` 承载，两者互不影响。

use std::cell::Cell;
use std::fmt::Display;

use serde::{Deserialize, Serialize};

use super::terminal::NoTerminalStats;

/// 分数分布直方图的桶数（分数上界）
const MAX_SCORE: usize = 100_000;

/// 单个动作的搜索结果
///
/// 统计多次模拟的分数分布，支持均值、标准差与排名加权平均分。直方图紧耦合于
/// 加权口径（整数桶边界 + 排名比例），改动即改变排序结果，不可随意替换实现。
#[derive(Debug, Clone)]
pub struct ActionResult {
    /// 分数分布直方图：`distribution[score] = 出现次数`
    distribution: Vec<u32>,
    /// 模拟次数
    num: u32,
    /// 分数总和
    pub sum: f64,
    /// 分数平方和
    pub sum_sq: f64,
    /// 最小分数
    min_score: f64,
    /// 最大分数
    max_score: f64,
    /// 缓存的加权平均分：`(radical_factor, result)`
    cached_weighted: Cell<Option<(f64, f64)>>,
}

impl Default for ActionResult {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionResult {
    /// 创建空的搜索结果
    pub fn new() -> Self {
        Self {
            distribution: vec![0; MAX_SCORE],
            num: 0,
            sum: 0.0,
            sum_sq: 0.0,
            min_score: f64::MAX,
            max_score: f64::MIN,
            cached_weighted: Cell::new(None),
        }
    }

    /// 加入一次模拟结果
    ///
    /// 分数按整数桶归档：负分落 0 桶、超过上界落末桶（与加权口径一致）。
    pub fn add(&mut self, score: f64) {
        self.num += 1;
        self.sum += score;
        self.sum_sq += score * score;
        self.min_score = self.min_score.min(score);
        self.max_score = self.max_score.max(score);
        let idx = (score as usize).clamp(0, MAX_SCORE - 1);
        self.distribution[idx] += 1;
        self.cached_weighted.set(None);
    }

    /// 模拟次数
    pub fn count(&self) -> u32 {
        self.num
    }

    /// 算术平均值；无样本时返回 `0`
    pub fn mean(&self) -> f64 {
        if self.num == 0 {
            return 0.0;
        }
        self.sum / self.num as f64
    }

    /// 样本标准差（贝塞尔校正）；样本数 ≤ 1 时返回 `0`
    pub fn stdev(&self) -> f64 {
        if self.num <= 1 {
            return 0.0;
        }
        let n = self.num as f64;
        let variance = (self.sum_sq - self.sum * self.sum / n) / (n - 1.0);
        variance.max(0.0).sqrt()
    }

    /// 排名加权平均分：按 `rank_ratio^radical_factor` 加权，越大越偏高分尾部
    ///
    /// `radical_factor = 0` 时退化为普通均值。结果按 `radical_factor` 缓存，
    /// 同一次搜索内多候选共用同一因子，故每个候选只算一次。
    pub fn weighted_mean(&self, radical_factor: f64) -> f64 {
        if self.num == 0 {
            return 0.0;
        }
        if radical_factor.abs() < 1e-6 {
            return self.mean();
        }
        if let Some((cached_rf, cached_result)) = self.cached_weighted.get() {
            if (cached_rf - radical_factor).abs() < 1e-9 {
                return cached_result;
            }
        }
        let result = self.compute_weighted_mean(radical_factor);
        self.cached_weighted.set(Some((radical_factor, result)));
        result
    }

    /// 不含缓存的加权平均分计算
    ///
    /// `rank_ratio` 取「累计到当前桶的样本比例」，桶内样本按中点计权。
    fn compute_weighted_mean(&self, radical_factor: f64) -> f64 {
        let n_inv = 1.0 / self.num as f64;
        let mut cumulative = 0.0;
        let mut weighted_sum = 0.0;
        let mut weight_total = 0.0;

        let end = (self.max_score as usize).min(MAX_SCORE - 1);
        for (score, &count) in self.distribution[..=end].iter().enumerate() {
            if count == 0 {
                continue;
            }
            let c = count as f64;
            let rank_ratio = (cumulative + 0.5 * c) * n_inv;
            let weight = rank_ratio.powf(radical_factor);
            weighted_sum += weight * c * score as f64;
            weight_total += weight * c;
            cumulative += c;
        }

        if weight_total > 0.0 {
            weighted_sum / weight_total
        } else {
            self.mean()
        }
    }

    /// 最小分数；无样本时返回 `0`
    pub fn min(&self) -> f64 {
        if self.num == 0 { 0.0 } else { self.min_score }
    }

    /// 最大分数；无样本时返回 `0`
    pub fn max(&self) -> f64 {
        if self.num == 0 { 0.0 } else { self.max_score }
    }
}

/// 按 rollout 序号对齐的原始分
///
/// 教师数据采集用：一次搜索内各候选的第 k 次 rollout 共享 CRN 种子，只有按下标
/// 对齐才能做 cross-fitting、对抗 winner curse。失败序号必须留 `None`，不能从
/// `Vec` 里删掉，否则后续元素会与其他候选错位。
#[derive(Debug, Clone)]
pub struct OrderedRollouts {
    /// 本次搜索的根种子（CRN 起点）
    pub root_seed: u64,
    /// 候选下标 → 按 rollout 序号的原始分（失败为内层 `None`）
    pub per_candidate: Vec<Vec<Option<f64>>>,
}

/// 搜索输出
///
/// 第二个类型参数是终局统计（不参与排序）：未接入观测的剧本为 ZST
/// [`NoTerminalStats`]，占用为零——注意**不是**空 `Vec`。
#[derive(Debug, Clone)]
pub struct SearchOutput<A, D = NoTerminalStats> {
    /// 动作列表
    pub actions: Vec<A>,
    /// 各动作的搜索结果（下标与 `actions` 对齐）
    pub action_results: Vec<ActionResult>,
    /// 各动作的终局多维统计（下标与 `actions` 对齐）
    pub terminal_results: Vec<D>,
    /// 最优动作索引（按加权平均分）
    pub best_action_idx: usize,
    /// 本次搜索使用的激进度因子
    pub radical_factor: f64,
    /// 按 rollout 序号保留的有序原始分；未开启采集时为 `None`
    pub ordered_rollouts: Option<OrderedRollouts>,
}

/// 手写而非 `derive(Default)`：后者会给泛型参数加上多余的 `A: Default` 约束，
/// 而空 `Vec<A>` 本就不需要 `A` 可默认构造。
impl<A, D> Default for SearchOutput<A, D> {
    fn default() -> Self {
        Self {
            actions: Vec::new(),
            action_results: Vec::new(),
            terminal_results: Vec::new(),
            best_action_idx: 0,
            radical_factor: 0.0,
            ordered_rollouts: None,
        }
    }
}

impl<A, D> SearchOutput<A, D> {
    /// 创建不带终局统计的搜索输出
    pub fn new(actions: Vec<A>, action_results: Vec<ActionResult>, radical_factor: f64) -> Self {
        Self::with_terminals(actions, action_results, Vec::new(), radical_factor)
    }

    /// 创建带终局多维统计的搜索输出（顺带算出最优动作下标）
    pub fn with_terminals(
        actions: Vec<A>,
        action_results: Vec<ActionResult>,
        terminal_results: Vec<D>,
        radical_factor: f64,
    ) -> Self {
        let best_action_idx = action_results
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                let wa = a.weighted_mean(radical_factor);
                let wb = b.weighted_mean(radical_factor);
                wa.partial_cmp(&wb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(0);

        Self {
            actions,
            action_results,
            terminal_results,
            best_action_idx,
            radical_factor,
            ordered_rollouts: None,
        }
    }

    /// 获取最优动作
    pub fn best_action(&self) -> &A {
        &self.actions[self.best_action_idx]
    }

    /// 获取最优动作的搜索结果
    pub fn best_result(&self) -> &ActionResult {
        &self.action_results[self.best_action_idx]
    }
}

impl<A: Display, D> SearchOutput<A, D> {
    /// 各动作的均分 / 加权均分 / 标准差（供上层输出与对照）
    pub fn to_scores(&self) -> Vec<ScoreEntry> {
        self.actions
            .iter()
            .enumerate()
            .map(|(i, action)| {
                let r = &self.action_results[i];
                ScoreEntry {
                    action: action.to_string(),
                    radical_factor: self.radical_factor,
                    count: r.count() as i64,
                    mean: r.mean(),
                    weighted_mean: r.weighted_mean(self.radical_factor),
                    stdev: r.stdev(),
                }
            })
            .collect()
    }
}

/// 动作的分数统计条目
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreEntry {
    /// 动作的展示文本
    pub action: String,
    /// 统计所用的激进度因子
    pub radical_factor: f64,
    /// 样本数
    pub count: i64,
    /// 算术均值
    pub mean: f64,
    /// 排名加权均值
    pub weighted_mean: f64,
    /// 样本标准差
    pub stdev: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：等频双峰按排名加权，桶边界沿用钳位口径
    #[test]
    fn test_weighted_mean_two_score_groups() {
        let mut result = ActionResult::new();
        result.add(20_000.0);
        result.add(80_000.0);
        assert_eq!(result.weighted_mean(0.0), 50_000.0, "激进度 0 使用普通均值");
        assert_eq!(result.weighted_mean(1.0), 65_000.0, "等频双峰按排名加权");

        // 负分落 0 桶、超上界落末桶：两个边界值都必须进入统计
        let mut clipped = ActionResult::new();
        clipped.add(0.0);
        clipped.add(110_000.0);
        assert_eq!(clipped.weighted_mean(1.0), 74_999.25);
    }

    /// 层1：退化样本取中性值，不 panic
    #[test]
    fn test_degenerate() {
        let empty = ActionResult::new();
        assert_eq!(empty.count(), 0);
        assert_eq!(empty.mean(), 0.0);
        assert_eq!(empty.stdev(), 0.0);
        assert_eq!(empty.weighted_mean(1.0), 0.0);
        assert_eq!(empty.min(), 0.0);
        assert_eq!(empty.max(), 0.0);

        let mut one = ActionResult::new();
        one.add(7.0);
        assert_eq!(one.count(), 1);
        assert_eq!(one.mean(), 7.0);
        assert_eq!(one.stdev(), 0.0);
        assert_eq!(one.min(), 7.0);
        assert_eq!(one.max(), 7.0);
    }

    /// 层1：最优动作按下标选出，终局统计与动作同长
    #[test]
    fn test_best_action_idx() {
        let mut low = ActionResult::new();
        low.add(10.0);
        let mut high = ActionResult::new();
        high.add(90.0);
        let out = SearchOutput::with_terminals(
            vec!["low", "high"],
            vec![low, high],
            vec![NoTerminalStats, NoTerminalStats],
            1.0,
        );
        assert_eq!(out.best_action_idx, 1);
        assert_eq!(*out.best_action(), "high");
        assert_eq!(out.best_result().mean(), 90.0);
        assert_eq!(out.terminal_results.len(), out.actions.len());
    }
}
