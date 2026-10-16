//! L3 端到端：`BasicGame` 固定种子跑满 78 回合（可复现性 + 不变量 + 基线数值）
//!
//! 数值基线（seed 42 / RaceFirstTrainer，供 L4 baseline 阶段固化参照）：
//! score / PT / 五维打印于测试输出；可复现性用「同种子跑两次逐项比对」守门。
//! 夹具统一取自 `umaai_tests::testing`（与配置默认层同参数）。

use anyhow::Result;
use rand::SeedableRng;
use rand::rngs::StdRng;
use umaai_core::Data;
use umaai_core::testing::data_root;
use umaai_tests::Checker;
use umaai_tests::testing::{RaceFirstTrainer, basic_newgame};
use umasim::game::Game;

/// 跑一局并返回终局快照（score / pt / five / turn）
fn run_once() -> Result<(i32, i32, [i32; 5], i32)> {
    let mut game = basic_newgame()?;
    let trainer = RaceFirstTrainer;
    let mut rng = StdRng::seed_from_u64(42);
    game.run_full_game(&trainer, &mut rng)?;
    Ok((
        game.uma.calc_score()?,
        game.uma.total_pt()?,
        game.uma.five_status,
        game.turn(),
    ))
}

/// 端到端：跑满 78 回合 + 可复现性 + 不变量
#[test]
fn basic_game_full_run() -> Result<()> {
    Data::init(data_root()?)?;
    let mut c = Checker::new("BasicGame 固定种子端到端");

    let (score, pt, five, turn) = run_once()?;
    c.check(turn == 77, "跑满 78 回合（0-based 末回合 77）");
    c.check(score > 0, "终局评分为正");
    c.check(pt > 0, "终局 PT 为正");
    c.check(five.iter().all(|&s| s >= 0), "五维非负");
    // 不变量：五维不超过上限
    {
        let game = basic_newgame()?;
        let limits = game.uma.five_status_limit;
        c.check(five.iter().zip(limits).all(|(&s, l)| s <= l), "五维不超过上限");
    }
    // 可复现性：同种子再跑一次逐项一致
    let (score2, pt2, five2, turn2) = run_once()?;
    c.check(
        (score, pt, five, turn) == (score2, pt2, five2, turn2),
        "同种子两次运行逐项一致",
    );

    println!(
        "基线参考: score={} PT={} five={:?} (seed=42, RaceFirstTrainer)",
        score, pt, five
    );
    c.finish()
}

/// 端到端：不同种子的结果可以有差异（随机性来源存在，非硬编码）
#[test]
fn basic_game_seed_varies() -> Result<()> {
    Data::init(data_root()?)?;
    let mut c = Checker::new("BasicGame 种子差异性");

    let mut game = basic_newgame()?;
    game.run_full_game(&RaceFirstTrainer, &mut StdRng::seed_from_u64(42))?;
    let a = game.uma.calc_score()?;

    let mut game = basic_newgame()?;
    game.run_full_game(&RaceFirstTrainer, &mut StdRng::seed_from_u64(4242))?;
    let b = game.uma.calc_score()?;

    // 事件抽签 / 训练失败等随机路径应导致不同终局（概率上几乎必然）
    c.check(a != b, "不同种子终局评分不同");
    println!("seed42={} seed4242={}", a, b);
    c.finish()
}
