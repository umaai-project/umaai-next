//! crate 内共享测试夹具（仅单元测试可见）
//!
//! 发布构建剔除本模块；跨 crate 集成测试的夹具见 `umaai-tests`。

use anyhow::{Result, bail};
use rand::SeedableRng;
use rand::rngs::StdRng;
use umaai_core::Data;
use umaai_core::gamedata::EventChoice;
use umaai_core::testing::data_root;

use crate::game::base::basic::{BasicAction, BasicGame};
use crate::game::{Game, InheritInfo, Trainer, TurnStage};

/// 比赛等级表夹具（与旧仓默认 72 项一致）
pub const RACE_GRADES: [i32; 72] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 3, 4, 3, 3, 4, 3, 3, 2, 3, 1, 1, 3, 4, 3, 4, 2, 2, 1, 2, 1, 1, 1, 1, 1, 3,
    3, 3, 3, 1, 1, 1, 1, 1, 2, 1, 2, 3, 1, 1, 2, 1, 2, 1, 1, 2, 1, 1, 3, 3, 3, 3, 3, 1, 2, 1, 1, 1, 2, 1,
];

/// 基础局夹具的马娘 ID（与 `umaai-tests` 的端到端夹具同参数）
pub const UMA_ID: u32 = 101901;

/// 基础局夹具卡组
pub const DECK: [u32; 6] = [302424, 302464, 302484, 302564, 302574, 302644];

/// 建立一局基础局（幂等初始化数据层 + 固定马娘 / 卡组 / 继承）
pub fn basic_game() -> Result<BasicGame> {
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

/// 只用于推进阶段的训练员：事件选项恒选第 0 组（未走到 Train 阶段不会被调用）
struct NoChoiceTrainer;

impl Trainer<BasicGame> for NoChoiceTrainer {
    fn select_action(&self, _game: &BasicGame, _actions: &[BasicAction], _rng: &mut StdRng) -> Result<usize> {
        Ok(0)
    }

    fn select_choice(&self, _game: &BasicGame, _choices: &[Vec<EventChoice>], _rng: &mut StdRng) -> Result<usize> {
        Ok(0)
    }
}

/// 把局面推进到首回合的**训练决策点**，返回该局面与候选动作
///
/// 搜索的生产调用点就在决策点（`Train` 阶段、人头分布已完成）。直接用
/// [`basic_game`] 的初始局面跑训练动作会读到尚未分配的人头分布，不是合法搜索根。
pub fn decision_root() -> Result<(BasicGame, Vec<BasicAction>)> {
    let mut game = basic_game()?;
    let mut rng = StdRng::seed_from_u64(0);
    let trainer = NoChoiceTrainer;
    for _ in 0..16 {
        if game.stage == TurnStage::Train {
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
