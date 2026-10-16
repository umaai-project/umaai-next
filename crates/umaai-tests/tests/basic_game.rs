//! L3 端到端：`BasicGame` 固定种子跑满 78 回合（可复现性 + 不变量 + 基线数值）
//!
//! 数值基线（seed 42 / RaceFirstTrainer，供 L4 baseline 阶段固化参照）：
//! score / PT / 五维打印于测试输出；可复现性用「同种子跑两次逐项比对」守门。

use anyhow::Result;
use rand::SeedableRng;
use rand::rngs::StdRng;
use umaai_core::Data;
use umaai_core::testing::data_root;
use umaai_tests::Checker;
use umasim::game::base::BaseAction;
use umasim::game::base::basic::{BasicAction, BasicGame};
use umasim::game::{Game, InheritInfo, Trainer};

/// 比赛等级表夹具（与旧仓默认 72 项一致，仅测试用）
const RACE_GRADES: [i32; 72] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 3, 4, 3, 3, 4, 3, 3, 2, 3, 1, 1, 3, 4, 3, 4, 2, 2, 1, 2, 1, 1, 1, 1, 1, 3,
    3, 3, 3, 1, 1, 1, 1, 1, 2, 1, 2, 3, 1, 1, 2, 1, 2, 1, 1, 2, 1, 1, 3, 3, 3, 3, 3, 1, 2, 1, 1, 1, 2, 1,
];

/// 马娘与卡组（与旧仓 basic 冒烟同参数，便于人工对照）
const UMA_ID: u32 = 101901;
const DECK: [u32; 6] = [302424, 302464, 302484, 302564, 302574, 302644];

/// 测试用训练员：Race > Clinic > Sleep > 其他（保证跑满），选项恒选第 0 组
struct RaceFirstTrainer;

impl Trainer<BasicGame> for RaceFirstTrainer {
    fn select_action(&self, _game: &BasicGame, actions: &[BasicAction], _rng: &mut StdRng) -> Result<usize> {
        let pri = |a: &BasicAction| match a.0 {
            BaseAction::Race => 0,
            BaseAction::Clinic => 1,
            BaseAction::Sleep => 2,
            _ => 3,
        };
        Ok(actions
            .iter()
            .enumerate()
            .min_by_key(|(_, a)| pri(a))
            .map(|(i, _)| i)
            .unwrap_or(0))
    }
    fn select_choice(
        &self,
        _game: &BasicGame,
        _choices: &[Vec<umaai_core::gamedata::EventChoice>],
        _rng: &mut StdRng,
    ) -> Result<usize> {
        Ok(0)
    }
}

fn newgame() -> Result<BasicGame> {
    BasicGame::newgame(
        UMA_ID,
        &DECK,
        InheritInfo {
            blue_count: [15, 3, 0, 0, 0],
            extra_count: [0, 30, 0, 0, 30, 30],
        },
        &RACE_GRADES,
    )
}

/// 跑一局并返回终局快照（score / pt / five / turn）
fn run_once() -> Result<(i32, i32, [i32; 5], i32)> {
    let mut game = newgame()?;
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
        let game = newgame()?;
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

    let mut game = newgame()?;
    game.run_full_game(&RaceFirstTrainer, &mut StdRng::seed_from_u64(42))?;
    let a = game.uma.calc_score()?;

    let mut game = newgame()?;
    game.run_full_game(&RaceFirstTrainer, &mut StdRng::seed_from_u64(4242))?;
    let b = game.uma.calc_score()?;

    // 事件抽签 / 训练失败等随机路径应导致不同终局（概率上几乎必然）
    c.check(a != b, "不同种子终局评分不同");
    println!("seed42={} seed4242={}", a, b);
    c.finish()
}
