//! L3：随机流分层的守门用例（P6e′）
//!
//! 这三条是"分流真的成立"的判据，不是跑通即过：
//!
//! 1. **决策流与规则流互不污染**——决策流多抽若干次，局面/事件逐位不变；
//! 2. **fork 语义**——同一 rollout 种子派出的分支共享同一套规则随机（搜索与 fork 工具的配对前提）；
//! 3. **未注入主种子即报错**——不静默回退到"单流混用"。
//!
//! 旧仓的对应资产是 `game/ramen/rng_consistency.rs`（RNG 受控重构 v2 的层 2 / 层 3），
//! 这里按新引擎的通用形态重写：不再依赖具体剧本的角标 / 分身 / PT 档位。

use anyhow::{Result, bail};
use rand::RngCore;
use rand::SeedableRng;
use rand::rngs::StdRng;
use umaai_core::Data;
use umaai_core::gamedata::EventChoice;
use umaai_core::testing::data_root;
use umaai_tests::Checker;
use umaai_tests::testing::{RACE_GRADES, RaceFirstTrainer, basic_newgame};
use umasim::game::base::basic::{BasicAction, BasicGame};
use umasim::game::{Game, InheritInfo, Trainer};
use umasim::search::FlatSearchGame;

/// 观察用回合数（0 基）
///
/// 必须是**非比赛回合**：比赛回合在 `reset_distribution()` 里清空人头分布，拿它做比较
/// 就变成"空 vs 空"。等级表里首场比赛落在 0 基回合 11（显示的第 12 回合），故取 10；
/// 用例另有一条断言拿等级表自证这一点，避免常量以后被人改到比赛回合并静默退化。
const TURNS: i32 = 10;

/// 决策流消耗扰动：每次决策前多抽这么多次
const EXTRA_DRAWS: usize = 5;

/// 夹具马娘 / 卡组（与端到端用例同参数）
const UMA_ID: u32 = 100101;
const DECK: [u32; 6] = [302424, 302464, 302484, 302564, 302574, 302644];

/// 在决策前多抽若干次决策流的训练员（其余行为与参考训练员一致）
///
/// 用来模拟"决策侧多消耗随机数"：若规则随机还挂在同一条流上，局面就会整体位移。
struct NoisyTrainer {
    /// 决策前额外抽签次数
    extra: usize,
}

impl Trainer<BasicGame> for NoisyTrainer {
    fn select_action(&self, game: &BasicGame, actions: &[BasicAction], rng: &mut StdRng) -> Result<usize> {
        for _ in 0..self.extra {
            rng.next_u64();
        }
        RaceFirstTrainer.select_action(game, actions, rng)
    }

    fn select_choice(&self, game: &BasicGame, choices: &[Vec<EventChoice>], rng: &mut StdRng) -> Result<usize> {
        for _ in 0..self.extra {
            rng.next_u64();
        }
        RaceFirstTrainer.select_choice(game, choices, rng)
    }
}

/// 推进到第 `limit` 回合（含）之后停下，返回走过的阶段数
fn advance(game: &mut BasicGame, trainer: &impl Trainer<BasicGame>, rng: &mut StdRng, limit: i32) -> Result<usize> {
    let mut stages = 0;
    loop {
        if game.turn() >= limit {
            return Ok(stages);
        }
        game.run_stage(trainer, rng)?;
        stages += 1;
        if !game.next() {
            bail!("推进到第 {limit} 回合之前对局已结束");
        }
    }
}

#[test]
fn rule_streams_survive_decision_draws() -> Result<()> {
    Data::init(data_root()?)?;
    let mut c = Checker::new("随机流分层：决策流消耗不影响局面/事件");

    let master = 20261017u64;
    let mut clean = basic_newgame()?;
    clean.set_rule_master(master);
    let mut noisy = basic_newgame()?;
    noisy.set_rule_master(master);

    // 两边的决策流种子相同，唯一差别是 noisy 每次决策前多抽 EXTRA_DRAWS 次
    let mut rng_clean = StdRng::seed_from_u64(master);
    let mut rng_noisy = StdRng::seed_from_u64(master);
    let stages_clean = advance(&mut clean, &RaceFirstTrainer, &mut rng_clean, TURNS)?;
    let stages_noisy = advance(&mut noisy, &NoisyTrainer { extra: EXTRA_DRAWS }, &mut rng_noisy, TURNS)?;

    println!("阶段数：干净 {stages_clean} / 多抽 {stages_noisy}");
    println!("干净局面：turn={} 分布={:?}", clean.turn(), clean.distribution);
    println!("多抽局面：turn={} 分布={:?}", noisy.turn(), noisy.distribution);

    c.check(stages_clean == stages_noisy, "两侧推进的阶段数一致");
    c.check(
        RACE_GRADES[TURNS as usize] == 0,
        "观察回合不是比赛回合（比空比较更硬的前提）",
    );
    c.check(!clean.is_race_turn(), "观察到的局面确实不在比赛回合");
    let assigned: usize = clean.distribution.iter().map(Vec::len).sum();
    c.check(
        clean.distribution.len() == 5 && assigned > 0,
        &format!("人头分布已分配 {assigned} 人（守门不许退化成空比较）"),
    );
    c.check(
        clean.distribution == noisy.distribution,
        "人头分布逐位一致（不吃决策流）",
    );
    c.check(clean.events == noisy.events, "事件计数逐位一致（不吃决策流）");
    c.check(
        clean.uma.five_status == noisy.uma.five_status && clean.uma.skill_pt == noisy.uma.skill_pt,
        "五维与 PT 逐位一致",
    );
    c.check(clean.base == noisy.base, "整个局面底座逐位一致（含两条规则流状态）");
    c.finish()
}

#[test]
fn fork_branches_share_rule_streams() -> Result<()> {
    Data::init(data_root()?)?;
    let mut c = Checker::new("随机流分层：fork 分支共享规则随机");

    let root = basic_newgame()?;
    let rollout_seed = 7u64;
    let mut a = root.fork_for_rollout(rollout_seed);
    let mut b = root.fork_for_rollout(rollout_seed);

    c.check(
        a.rule_master() == Some(rollout_seed),
        "fork 把 rollout 种子注入为主种子",
    );
    c.check(b.rule_master() == Some(rollout_seed), "同种子的另一支同样注入");

    // 同一决策流种子推进同样步数：两支应逐位一致（这正是搜索配对的语义）
    let mut rng_a = StdRng::seed_from_u64(rollout_seed);
    let mut rng_b = StdRng::seed_from_u64(rollout_seed);
    advance(&mut a, &RaceFirstTrainer, &mut rng_a, TURNS)?;
    advance(&mut b, &RaceFirstTrainer, &mut rng_b, TURNS)?;
    c.check(a.base == b.base, "两支局面逐位一致");
    c.check(a.stage == b.stage, "两支阶段位置一致");

    // 换一个 rollout 种子，规则随机应当不同（否则配对退化成同一条轨迹）
    let mut other = root.fork_for_rollout(rollout_seed + 1);
    let mut rng_other = StdRng::seed_from_u64(rollout_seed);
    advance(&mut other, &RaceFirstTrainer, &mut rng_other, TURNS)?;
    c.check(
        other.distribution != a.distribution || other.base != a.base,
        "换 rollout 种子应换一条规则随机",
    );
    c.finish()
}

#[test]
fn missing_master_is_rejected() -> Result<()> {
    Data::init(data_root()?)?;
    let mut c = Checker::new("随机流分层：未注入主种子即报错");

    // 直接建局、不注入主种子
    let mut game = BasicGame::newgame(UMA_ID, &DECK, InheritInfo::default(), &RACE_GRADES)?;
    c.check(game.rule_master().is_none(), "新局尚无主种子");

    let err = game
        .run_stage(&RaceFirstTrainer, &mut StdRng::seed_from_u64(1))
        .expect_err("未注入主种子应拒绝推进");
    let msg = format!("{err:#}");
    println!("拒绝原因: {msg}");
    c.check(msg.contains("规则主种子未注入"), "错误文案指明主种子未注入");
    c.check(msg.contains("set_rule_master"), "错误文案给出注入入口");

    // 注入后同一步应能推进（报错的是"没注入"，不是别的）
    game.set_rule_master(42);
    c.check(
        game.run_stage(&RaceFirstTrainer, &mut StdRng::seed_from_u64(1)).is_ok(),
        "注入后即可推进",
    );
    c.finish()
}
