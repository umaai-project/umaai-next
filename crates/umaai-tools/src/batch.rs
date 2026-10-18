//! 固定种子批量跑局
//!
//! 输入取配置层（默认层 + 宿主覆盖层），命令行只覆盖局数与起始种子；跑局装配链与
//! `umaai-cli` 的入口**共用同一对装配函数**，故批量看到的数值与单局冒烟同源。
//! 第 i 局用 `seed + i`，同参数重复运行逐位一致。

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use umaai::Config;
use umaai::runner::{Runner, Summary};
use umaai_cli::cli::{build_game, build_trainer};
use umaai_core::stats;

use crate::setup::NullFrontend;

/// 跑批规格
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    /// 局数
    pub runs: u32,
    /// 起始种子（第 i 局用 `seed + i`）
    pub seed: u64,
}

impl Spec {
    /// 取配置层的 `sim.runs` / `sim.seed`
    #[must_use]
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            runs: cfg.sim.runs,
            seed: cfg.sim.seed,
        }
    }
}

/// 单局结果
#[derive(Debug, Clone)]
pub struct RunResult {
    /// 第几局（0 基）
    pub index: u32,
    /// 本局种子
    pub seed: u64,
    /// 终局汇总
    pub summary: Summary,
    /// 本局墙钟耗时
    pub elapsed: Duration,
}

/// 评分分布（逐局终局评分与 PT）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Distribution {
    /// 样本数
    pub count: u32,
    /// 评分均值
    pub mean: f64,
    /// 评分中位数
    pub median: f64,
    /// 评分最小值
    pub min: f64,
    /// 评分最大值
    pub max: f64,
    /// 评分样本标准差（样本数 < 2 时为 0）
    pub stdev: f64,
    /// PT 均值
    pub pt_mean: f64,
}

/// 跑批汇总
#[derive(Debug, Clone)]
pub struct Report {
    /// 逐局结果（按局号升序）
    pub runs: Vec<RunResult>,
    /// 评分分布
    pub dist: Distribution,
    /// 整批墙钟耗时
    pub elapsed: Duration,
}

impl Report {
    /// 每秒局数（耗时为 0 时返回 0，不产生无穷）
    #[must_use]
    pub fn throughput(&self) -> f64 {
        let secs = self.elapsed.as_secs_f64();
        if secs <= 0.0 {
            return 0.0;
        }
        self.runs.len() as f64 / secs
    }
}

/// 按规格跑 `spec.runs` 局，返回逐局结果与分布
///
/// 单局失败即中断并指明局号：批量里的失败不该被吞掉（失败率是量具本身要看的量）。
pub fn run(cfg: &Config, spec: &Spec) -> Result<Report> {
    if spec.runs == 0 {
        anyhow::bail!("跑批局数必须 ≥ 1");
    }
    let started = Instant::now();
    let mut runs = Vec::with_capacity(spec.runs as usize);
    for index in 0..spec.runs {
        runs.push(run_one(cfg, index, spec.seed + u64::from(index))?);
    }
    let elapsed = started.elapsed();
    let dist = distribution(&runs);
    Ok(Report { runs, dist, elapsed })
}

/// 跑一局（种子按局号覆盖，其余参数沿用配置）
fn run_one(cfg: &Config, index: u32, seed: u64) -> Result<RunResult> {
    let mut one = cfg.clone();
    one.sim.seed = seed;
    one.sim.runs = 1;

    let started = Instant::now();
    let game = build_game(&one).with_context(|| format!("第 {index} 局装配失败"))?;
    let trainer = build_trainer(&one).with_context(|| format!("第 {index} 局训练员装配失败"))?;
    let mut fe = NullFrontend;
    let mut runner = Runner::new(&one, &mut fe)?;
    let summary = runner
        .run_game(game, &trainer, None)
        .with_context(|| format!("第 {index} 局运行失败（seed {seed}）"))?;
    Ok(RunResult {
        index,
        seed,
        summary,
        elapsed: started.elapsed(),
    })
}

/// 从逐局结果算评分分布
fn distribution(runs: &[RunResult]) -> Distribution {
    let scores: Vec<f64> = runs.iter().map(|r| f64::from(r.summary.score)).collect();
    let pts: Vec<f64> = runs.iter().map(|r| f64::from(r.summary.pt)).collect();
    let (min, max) = stats::min_max(&scores).unwrap_or((0.0, 0.0));
    Distribution {
        count: u32::try_from(runs.len()).unwrap_or(u32::MAX),
        mean: stats::mean(&scores),
        median: stats::median(&scores).unwrap_or(0.0),
        min,
        max,
        stdev: stats::stdev(&scores),
        pt_mean: stats::mean(&pts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一条逐局结果（只填分布关心的字段）
    fn result(index: u32, score: i32, pt: i32) -> RunResult {
        RunResult {
            index,
            seed: u64::from(index),
            summary: Summary {
                score,
                pt,
                rank: String::new(),
                five_status: [0; 5],
                turn: 0,
                stages: 0,
                decisions: 0,
            },
            elapsed: Duration::ZERO,
        }
    }

    /// 层1：分布取自逐局评分，样本数与极值对齐
    #[test]
    fn test_distribution() {
        let runs = vec![result(0, 100, 10), result(1, 200, 30), result(2, 300, 20)];
        let d = distribution(&runs);
        assert_eq!(d.count, 3);
        assert_eq!(d.mean, 200.0);
        assert_eq!(d.median, 200.0);
        assert_eq!(d.min, 100.0);
        assert_eq!(d.max, 300.0);
        assert_eq!(d.stdev, 100.0);
        assert_eq!(d.pt_mean, 20.0);
    }

    /// 层1：单局样本的标准差为 0（不产生 NaN）
    #[test]
    fn test_distribution_single_sample() {
        let d = distribution(&[result(0, 42, 7)]);
        assert_eq!(d.count, 1);
        assert_eq!(d.stdev, 0.0);
        assert_eq!(d.mean, 42.0);
        assert_eq!(d.min, d.max);
    }

    /// 层1：空样本一律中性值
    #[test]
    fn test_distribution_empty() {
        let d = distribution(&[]);
        assert_eq!(d.count, 0);
        assert_eq!(d.mean, 0.0);
        assert_eq!(d.min, 0.0);
    }

    /// 层1：规格缺省取配置层，零局报错
    #[test]
    fn test_spec_guards() {
        let s = Spec { runs: 3, seed: 42 };
        assert_eq!(s.runs, 3);
        assert_eq!(s.seed, 42);
    }

    /// 层1：吞吐在耗时为 0 时取 0
    #[test]
    fn test_throughput_zero_elapsed() {
        let rep = Report {
            runs: vec![result(0, 1, 1)],
            dist: distribution(&[]),
            elapsed: Duration::ZERO,
        };
        assert_eq!(rep.throughput(), 0.0);
    }
}
