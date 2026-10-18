//! 多臂同种子配对跑批内核（测量函数由调用方注入）
//!
//! 只做测量编排：给定一组卡组与**同一段种子序列**，逐臂跑批、以首臂为基准逐位配对，
//! 不碰任何具体局面。测量函数是闭包 `measure(deck, seed) -> 观测值`，因此：
//!
//! - 本仓基础局档传 [`crate::deck::measure`]（契约随机训练员）；
//! - 剧本仓可传自己的装配 + **剧本特化手写训练员**，复用同一套配对与统计口径。
//!
//! 口径：第 i 局用 `seed + i`（与 `batch` 同源）；异常局**不补采**（直接报错），
//! 保证各臂的配对键一一对应。

use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use umaai_core::stats::{self, Paired};

/// 反推所需局数时的默认目标分辨率
///
/// 依据本仓基础局实测：同类型单卡替换的效应量级约 25 分，故默认按「分辨 25 分」给建议。
pub const DEFAULT_GOAL: f64 = 25.0;

/// 跑批规格
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    /// 每臂局数
    pub runs: u32,
    /// 起始种子（第 i 局用 `seed + i`）
    pub seed: u64,
}

/// 一臂的逐局观测
#[derive(Debug, Clone, PartialEq)]
pub struct Arm {
    /// 卡组（idrank）
    pub deck: Vec<u32>,
    /// 逐局观测值（与 `Spec::runs` 等长、按局号升序）
    pub values: Vec<f64>,
}

impl Arm {
    /// 观测均值
    #[must_use]
    pub fn mean(&self) -> f64 {
        stats::mean(&self.values)
    }

    /// 观测样本标准差
    #[must_use]
    pub fn sd(&self) -> f64 {
        stats::stdev(&self.values)
    }

    /// 观测中位数
    #[must_use]
    pub fn median(&self) -> f64 {
        stats::median(&self.values).unwrap_or(0.0)
    }

    /// 观测 P10（升序样本的 10% 分位）
    #[must_use]
    pub fn p10(&self) -> f64 {
        let mut sorted = self.values.clone();
        sorted.sort_by(f64::total_cmp);
        stats::percentile(&sorted, 0.1)
    }
}

/// 一臂相对基准的配对结果
#[derive(Debug, Clone, PartialEq)]
pub struct ArmResult {
    /// 该臂的观测
    pub arm: Arm,
    /// 与基准臂的配对差分（`臂 − 基准`）
    pub paired: Paired,
    /// 按本臂 Δsd 反推：分辨 `goal` 大小的效应还需要多少配对数
    pub runs_for_goal: u32,
}

/// 多臂报告
#[derive(Debug, Clone)]
pub struct Report {
    /// 跑批规格
    pub spec: Spec,
    /// 反推局数用的目标分辨率
    pub goal: f64,
    /// 基准臂（入参首臂）
    pub base: Arm,
    /// 其余各臂（与入参顺序一致）
    pub results: Vec<ArmResult>,
    /// 整批墙钟耗时
    pub elapsed: Duration,
}

impl Report {
    /// 按配对增量降序给出臂下标（并列按卡组字典序）
    #[must_use]
    pub fn ranking(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.results.len()).collect();
        idx.sort_by(|a, b| {
            self.results[*b]
                .paired
                .mean
                .total_cmp(&self.results[*a].paired.mean)
                .then(self.results[*a].arm.deck.cmp(&self.results[*b].arm.deck))
        });
        idx
    }
}

/// 逐臂跑批并与首臂配对
///
/// `decks` 首项是**基准臂**；每臂跑 `spec.runs` 局（第 i 局 `spec.seed + i`），
/// 然后逐位配对。至少两臂（只有基准臂时没有可比对象）。
pub fn run<F>(spec: Spec, decks: &[Vec<u32>], goal: f64, mut measure: F) -> Result<Report>
where
    F: FnMut(&[u32], u64) -> Result<f64>,
{
    ensure!(spec.runs >= 1, "每臂局数必须 ≥ 1（收到 {}）", spec.runs);
    ensure!(decks.len() >= 2, "至少两臂（首臂为基准；收到 {}）", decks.len());
    let started = Instant::now();

    let mut arms = Vec::with_capacity(decks.len());
    for deck in decks {
        let mut values = Vec::with_capacity(spec.runs as usize);
        for i in 0..spec.runs {
            values.push(measure(deck, spec.seed + u64::from(i))?);
        }
        arms.push(Arm { deck: deck.clone(), values });
    }

    let mut iter = arms.into_iter();
    let base = iter.next().unwrap_or(Arm {
        deck: Vec::new(),
        values: Vec::new(),
    });
    let mut results = Vec::with_capacity(decks.len() - 1);
    for arm in iter {
        let paired = stats::paired(&base.values, &arm.values)?;
        let runs_for_goal = stats::paired_runs_for_effect(paired.sd, goal);
        results.push(ArmResult { arm, paired, runs_for_goal });
    }
    Ok(Report {
        spec,
        goal,
        base,
        results,
        elapsed: started.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 桩测量：观测值只依赖卡组首张与种子——配对差分恒定，单臂方差不为零
    fn stub(deck: &[u32], seed: u64) -> Result<f64> {
        let base = f64::from(deck.first().copied().unwrap_or(0));
        Ok(base * 10.0 + (seed % 7) as f64)
    }

    /// 层1：同种子配对把「与种子有关的共同波动」消掉，只留卡组差
    #[test]
    fn test_pairing_removes_common_noise() -> Result<()> {
        let spec = Spec { runs: 32, seed: 100 };
        let decks = vec![vec![1_u32], vec![2_u32], vec![1_u32]];
        let rep = run(spec, &decks, DEFAULT_GOAL, stub)?;
        assert_eq!(rep.base.deck, vec![1]);
        assert_eq!(rep.results.len(), 2);
        assert!(rep.base.sd() > 0.0, "单臂观测含与种子有关的波动");
        assert!((rep.results[0].paired.mean - 10.0).abs() < 1e-12, "换一张卡 = +10");
        assert_eq!(rep.results[0].paired.sd, 0.0, "配对差分应把共同波动消掉");
        assert_eq!(rep.results[0].paired.wins, spec.runs);
        assert!((rep.results[1].paired.mean - 0.0).abs() < 1e-12, "同一副卡组应给出零差");
        assert_eq!(rep.results[1].paired.ties, spec.runs);
        // 零 Δsd 无法反推局数（中性值 0）
        assert_eq!(rep.results[1].runs_for_goal, 0);
        Ok(())
    }

    /// 层1：排序按配对增量降序，并列按卡组字典序
    #[test]
    fn test_ranking() -> Result<()> {
        let spec = Spec { runs: 4, seed: 0 };
        let rep = run(
            spec,
            &[vec![1_u32], vec![3_u32], vec![2_u32], vec![1_u32]],
            DEFAULT_GOAL,
            stub,
        )?;
        let ranking = rep.ranking();
        assert_eq!(ranking, vec![0, 1, 2], "3 > 2 > 同卡（0 差）");
        Ok(())
    }

    /// 层1：臂数与局数守卫；测量失败即中断（异常局不补采）
    #[test]
    fn test_guards() {
        let spec = Spec { runs: 0, seed: 0 };
        assert!(run(spec, &[vec![1_u32], vec![2_u32]], DEFAULT_GOAL, stub).is_err());
        let spec = Spec { runs: 4, seed: 0 };
        assert!(run(spec, &[vec![1_u32]], DEFAULT_GOAL, stub).is_err(), "单臂无法配对");
        assert!(
            run(spec, &[vec![1_u32], vec![2_u32]], DEFAULT_GOAL, |_, seed| {
                if seed == 2 {
                    anyhow::bail!("模拟失败")
                } else {
                    Ok(1.0)
                }
            })
            .is_err(),
            "任一局失败即报错，不静默跳过"
        );
    }

    /// 层1：退化样本的均值 / 分位取中性值，不 panic
    #[test]
    fn test_arm_stats_degenerate() {
        let one = Arm {
            deck: vec![1],
            values: vec![5.0],
        };
        assert_eq!(one.mean(), 5.0);
        assert_eq!(one.sd(), 0.0);
        assert_eq!(one.median(), 5.0);
        assert_eq!(one.p10(), 5.0);
        let empty = Arm { deck: vec![], values: vec![] };
        assert_eq!(empty.mean(), 0.0);
        assert_eq!(empty.median(), 0.0);
    }
}
