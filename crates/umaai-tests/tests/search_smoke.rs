//! L3 端到端：`BasicGame` + `RandomTrainer` 的小规模扁平搜索
//!
//! 覆盖：固定种子可复现、每候选预算与探针计数一致、终局观测未接入时的口径。
//! 不设数值基线——基础局不带专用策略，分数不具跨版本可比性，只守结构与可复现性。

use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow, bail};
use rand::SeedableRng;
use rand::rngs::StdRng;
use umaai_core::Data;
use umaai_core::gamedata::EventChoice;
use umaai_core::testing::data_root;
use umaai_tests::Checker;
use umasim::game::base::basic::{BasicAction, BasicGame};
use umasim::game::base::stage::BasicStage;
use umasim::game::{Game, InheritInfo, Trainer};
use umasim::search::{FlatSearch, NoTerminal, SearchConfig, SearchProbe};

/// 比赛等级表夹具（与单元测试夹具同参数）
const RACE_GRADES: [i32; 72] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 3, 4, 3, 3, 4, 3, 3, 2, 3, 1, 1, 3, 4, 3, 4, 2, 2, 1, 2, 1, 1, 1, 1, 1, 3,
    3, 3, 3, 1, 1, 1, 1, 1, 2, 1, 2, 3, 1, 1, 2, 1, 2, 1, 1, 2, 1, 1, 3, 3, 3, 3, 3, 1, 2, 1, 1, 1, 2, 1,
];

/// 夹具马娘与卡组（与 `basic_game` 端到端用例同参数）
const UMA_ID: u32 = 101901;
const DECK: [u32; 6] = [302424, 302464, 302484, 302564, 302574, 302644];

/// 每候选搜索次数：小规模，只守结构与可复现性
const SEARCH_N: usize = 4;

/// 训练阶段的阶段键（`FlatSearchGame::crn_stage_key` 的显式映射）
const TRAIN_STAGE_KEY: u64 = 3;

fn newgame() -> Result<BasicGame> {
    Data::init(data_root()?)?;
    BasicGame::newgame(
        UMA_ID,
        &DECK,
        InheritInfo {
            blue_count: [15, 3, 0, 0, 0],
            extra_count: [0, 30, 0, 0, 30, 40],
        },
        &RACE_GRADES,
    )
}

/// 只用于推进阶段的训练员：事件选项恒选第 0 组
struct NoChoiceTrainer;

impl Trainer<BasicGame> for NoChoiceTrainer {
    fn select_action(&self, _game: &BasicGame, _actions: &[BasicAction], _rng: &mut StdRng) -> Result<usize> {
        Ok(0)
    }

    fn select_choice(&self, _game: &BasicGame, _choices: &[Vec<EventChoice>], _rng: &mut StdRng) -> Result<usize> {
        Ok(0)
    }
}

/// 推进到首回合的**训练决策点**并返回该局面与候选动作
///
/// 搜索的生产调用点就在决策点（Train 阶段、人头分布已完成）；直接用初始局面跑训练
/// 动作会读到尚未分配的人头分布。
fn decision_root() -> Result<(BasicGame, Vec<BasicAction>)> {
    let mut game = newgame()?;
    let mut rng = StdRng::seed_from_u64(0);
    let trainer = NoChoiceTrainer;
    for _ in 0..16 {
        if game.stage == BasicStage::Train {
            let actions = game.list_actions()?;
            return Ok((game, actions));
        }
        game.run_stage(&trainer, &mut rng)?;
        if !game.next() {
            bail!("首回合未抵达训练阶段");
        }
    }
    bail!("推进阶段次数超限，未抵达训练阶段")
}

/// 一次搜索的结构化结果（供检查器逐项比对）
struct Run {
    /// 中选动作下标
    best: usize,
    /// 各候选均分
    means: Vec<f64>,
    /// 终局统计条数
    terminal_len: usize,
    /// 成本探针快照
    probe: SearchProbe,
}

/// 用固定种子跑一次搜索（均匀分配，避免 UCB 的分配差异干扰可复现性判定）
fn run(seed: u64) -> Result<Run> {
    let (game, actions) = decision_root()?;
    let sink = Arc::new(Mutex::new(Vec::new()));
    let cfg = SearchConfig::default().with_ucb(false).with_search_n(SEARCH_N);
    let mut rng = StdRng::seed_from_u64(seed);
    let fs = FlatSearch::<BasicGame>::new(cfg).with_probe(sink.clone());
    let out = fs.search(&game, &actions, &mut rng, |_| NoTerminal)?;

    let mut probes = sink.lock().map_err(|e| anyhow!("探针锁中毒: {e}"))?;
    let probe = probes.pop().ok_or_else(|| anyhow!("探针未记录搜索结果"))?;
    Ok(Run {
        best: out.best_action_idx,
        means: out.action_results.iter().map(|r| r.mean()).collect(),
        terminal_len: out.terminal_results.len(),
        probe,
    })
}

#[test]
fn test_search_smoke() -> Result<()> {
    let a = run(42)?;
    let b = run(42)?;

    println!("中选下标 = {}", a.best);
    println!("各候选均分 = {:?}", a.means);
    println!(
        "探针: turn={} stage={} planned={:?} succeeded={:?} failed={:?} elapsed={:?}",
        a.probe.turn, a.probe.stage_key, a.probe.planned, a.probe.succeeded, a.probe.failed, a.probe.elapsed
    );

    let mut c = Checker::new("扁平搜索端到端（BasicGame + RandomTrainer）");
    c.check(!a.means.is_empty(), "候选非空");
    c.check(a.best < a.means.len(), "中选下标落在候选范围内");
    c.check(a.means.iter().all(|m| m.is_finite()), "各候选均分有限");
    c.check(a.terminal_len == a.means.len(), "终局统计条数与候选数一致");
    c.check(a.probe.candidates == a.means.len(), "探针候选数与输出一致");
    c.check(
        a.probe.planned.iter().all(|&p| p == SEARCH_N),
        "每候选计划次数 = search_n",
    );
    c.check(a.probe.succeeded == a.probe.planned, "无失败时成功次数等于计划次数");
    c.check(a.probe.total_failed() == 0, "基础局 rollout 不应失败");
    c.check(a.probe.stage_key == TRAIN_STAGE_KEY, "搜索发生在训练决策点");
    c.check(a.best == b.best && a.means == b.means, "同种子两次搜索逐项一致");
    c.finish()
}
