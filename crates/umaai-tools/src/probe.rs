//! 固定根搜索成本探针
//!
//! 量的是「**单决策点的一次搜索**」：局面与种子固定，搜索参数取配置层，输出候选数、
//! 每候选预算（计划 / 成功 / 失败）、中选动作、根局面评分与墙钟耗时。它是搜索成本
//! 跨 commit 对比最稳的量具——同一输入下除了机器耗时本身，其余全部逐位一致。
//!
//! 探针只**观测**，不改决策语义：成本走 [`FlatSearch::with_probe`]，不参与任何判据。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rand::SeedableRng;
use rand::rngs::StdRng;
use umaai::Config;
use umaai_cli::cli::build_game;
use umasim::game::Game;
use umasim::game::base::basic::{BasicAction, BasicGame};
use umasim::search::{FlatSearch, FlatSearchGame, NoTerminal, SearchConfig, SearchOutput, SearchProbe};

use crate::setup::decision_root;

/// 一次固定根搜索的观测结果
#[derive(Debug, Clone)]
pub struct Report {
    /// 根局面回合（0 基）
    pub turn: usize,
    /// 根局面的阶段键
    pub stage_key: u64,
    /// 进入搜索的候选数
    pub candidates: usize,
    /// 每候选计划的 rollout 次数
    pub planned: Vec<usize>,
    /// 每候选成功的 rollout 次数
    pub succeeded: Vec<usize>,
    /// 每候选失败的 rollout 次数
    pub failed: Vec<usize>,
    /// 加权均值口径下的中选下标
    pub best_idx: usize,
    /// 中选动作的展示文本
    pub best_action: String,
    /// 各候选均分（下标与候选对齐）
    pub means: Vec<f64>,
    /// 根局面的真实评分（不乘 PT 偏好折算率）
    pub root_score: f64,
    /// 内核墙钟耗时
    pub elapsed: Duration,
}

impl Report {
    /// 计划 rollout 总数
    #[must_use]
    pub fn total_planned(&self) -> usize {
        self.planned.iter().sum()
    }

    /// 成功 rollout 总数
    #[must_use]
    pub fn total_succeeded(&self) -> usize {
        self.succeeded.iter().sum()
    }

    /// 失败 rollout 总数
    #[must_use]
    pub fn total_failed(&self) -> usize {
        self.failed.iter().sum()
    }
}

/// 在首个训练决策点上跑一次搜索，返回成本与结果观测
///
/// `seed` 同时决定推进路径与搜索随机流；搜索预算与口径取配置层的 `search` 段。
pub fn search_root(cfg: &Config, seed: u64) -> Result<Report> {
    let mut game = build_game(cfg).context("探针局面装配失败")?;
    // 探针的种子同时作规则主种子：与搜索分支（fork 注入 rollout 种子）同一套语义
    game.set_rule_master(seed);
    let (game, actions) = decision_root(game, seed)?;
    run_search(&game, &actions, &cfg.search, seed)
}

/// 在给定局面上跑一次搜索并取观测（基线用例复用）
pub(crate) fn run_search(
    game: &BasicGame,
    actions: &[BasicAction],
    search_cfg: &SearchConfig,
    seed: u64,
) -> Result<Report> {
    let root_score = game.search_score(search_cfg.pt_favor_rate)?;
    let sink = Arc::new(Mutex::new(Vec::new()));
    let mut rng = StdRng::seed_from_u64(seed);
    let search = FlatSearch::<BasicGame>::new(search_cfg.clone()).with_probe(sink.clone());
    let out = search.search(game, actions, &mut rng, |_| NoTerminal)?;

    let probe = {
        let mut probes = sink.lock().map_err(|err| anyhow!("探针锁中毒: {err}"))?;
        probes.pop().ok_or_else(|| anyhow!("内核未记录成本探针"))?
    };
    Ok(report_from(&probe, &out, root_score))
}

/// 把内核探针与搜索结果拼成观测记录
fn report_from(probe: &SearchProbe, out: &SearchOutput<BasicAction>, root_score: f64) -> Report {
    Report {
        turn: probe.turn,
        stage_key: probe.stage_key,
        candidates: probe.candidates,
        planned: probe.planned.clone(),
        succeeded: probe.succeeded.clone(),
        failed: probe.failed.clone(),
        best_idx: out.best_action_idx,
        best_action: out.best_action().to_string(),
        means: out.action_results.iter().map(|r| r.mean()).collect(),
        root_score,
        elapsed: probe.elapsed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：总数由逐候选求和（不另存一份可能失配的合计）
    #[test]
    fn test_totals() {
        let r = Report {
            turn: 0,
            stage_key: 3,
            candidates: 2,
            planned: vec![4, 4],
            succeeded: vec![4, 3],
            failed: vec![0, 1],
            best_idx: 1,
            best_action: "训练".to_string(),
            means: vec![10.0, 20.0],
            root_score: 15.0,
            elapsed: Duration::from_millis(5),
        };
        assert_eq!(r.total_planned(), 8);
        assert_eq!(r.total_succeeded(), 7);
        assert_eq!(r.total_failed(), 1);
        assert_eq!(
            r.total_succeeded() + r.total_failed(),
            r.total_planned(),
            "成功 + 失败应等于计划数"
        );
    }
}
