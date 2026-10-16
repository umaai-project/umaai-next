//! L3 端到端：引擎 `Runner` 与 `Game::run_full_game` 的等价性
//!
//! 引擎层只做编排（事件下发 + 决策捕获），不得扰动模拟本身：同一局参数、同一种子
//! 下，经 `Runner` 跑出的终局必须与直接调用 provided 方法逐位一致。同时核对配置
//! 默认层与跨 crate 夹具同参数——默认层被改动会在这里暴露。

use anyhow::Result;
use rand::SeedableRng;
use rand::rngs::StdRng;
use umaai::{Config, Frontend, Runner};
use umaai_core::Data;
use umaai_core::testing::data_root;
use umaai_tests::Checker;
use umaai_tests::testing::{DECK, RACE_GRADES, RaceFirstTrainer, UMA_ID, basic_newgame, inherit};
use umasim::game::Game;

/// 只关心终局的空前端（所有事件都有默认空实现）
struct Silent;

impl Frontend for Silent {}

/// 直接路径的终局快照
fn direct_summary(seed: u64) -> Result<(i32, i32, [i32; 5], i32)> {
    let mut game = basic_newgame()?;
    game.run_full_game(&RaceFirstTrainer, &mut StdRng::seed_from_u64(seed))?;
    Ok((
        game.uma.calc_score()?,
        game.uma.total_pt()?,
        game.uma.five_status,
        game.turn(),
    ))
}

/// 端到端：两条路径终局逐位一致 + 配置默认层与夹具同参数
#[test]
fn runner_matches_direct_run() -> Result<()> {
    Data::init(data_root()?)?;
    let mut c = Checker::new("Runner 与 run_full_game 等价");

    let cfg = Config::load_from_data_root(data_root()?, None)?;
    c.check(cfg.sim.uma == UMA_ID, "默认层马娘与夹具一致");
    c.check(cfg.sim.cards == DECK.to_vec(), "默认层卡组与夹具一致");
    c.check(cfg.inherit() == inherit(), "默认层继承因子与夹具一致");
    c.check(
        cfg.race_grades()?.to_vec() == RACE_GRADES.to_vec(),
        "默认层等级表预设与夹具一致",
    );
    c.check(cfg.sim.seed == 42, "默认层种子为 42");

    let (score, pt, five, turn) = direct_summary(cfg.sim.seed)?;
    let mut fe = Silent;
    let summary = {
        let mut runner = Runner::new(&cfg, &mut fe)?;
        runner.run_game(basic_newgame()?, &RaceFirstTrainer, None)?
    };

    c.check(summary.turn == turn && summary.turn == 77, "回合数与直接路径一致且跑满");
    c.check(summary.score == score, "终局评分逐位一致");
    c.check(summary.pt == pt, "终局 PT 逐位一致");
    c.check(summary.five_status == five, "终局五维逐位一致");
    c.check(summary.stages > 0, "阶段计数为正");
    c.check(summary.decisions == 0, "未注入决策源时决策计数为 0");
    c.check(!summary.rank.is_empty(), "评分档次非空");

    println!(
        "基线参考: score={} PT={} five={:?} turn={} rank={} stages={}",
        summary.score, summary.pt, summary.five_status, summary.turn, summary.rank, summary.stages
    );
    c.finish()
}
