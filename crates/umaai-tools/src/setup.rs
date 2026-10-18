//! 局面构造与推进（工具件内部）
//!
//! 三个工具都要「先得到一个确定的决策点」，这件事收在本模块一处：推进阶段只认确定性
//! 的[首候选训练员](FirstActionTrainer)（动作与事件选项都取第 0 个候选），同一输入
//! 下根局面逐位一致。

use anyhow::{Result, bail};
use rand::SeedableRng;
use rand::rngs::StdRng;
use umaai_core::gamedata::EventChoice;
use umasim::game::base::basic::{BasicAction, BasicGame};
use umasim::game::base::stage::BasicStage;
use umasim::game::{Game, Trainer};

/// 推进阶段允许的最大步数（首回合的训练决策点远小于该值）
const MAX_ADVANCE: usize = 16;

/// 只用于推进阶段的确定性训练员：动作与事件选项都取第 0 个候选
pub(crate) struct FirstActionTrainer;

impl Trainer<BasicGame> for FirstActionTrainer {
    fn select_action(&self, _game: &BasicGame, _actions: &[BasicAction], _rng: &mut StdRng) -> Result<usize> {
        Ok(0)
    }

    fn select_choice(&self, _game: &BasicGame, _choices: &[Vec<EventChoice>], _rng: &mut StdRng) -> Result<usize> {
        Ok(0)
    }
}

/// 不接收任何事件的前端：批量与探针只要终局汇总，不要呈现
pub(crate) struct NullFrontend;

impl umaai::Frontend for NullFrontend {}

/// 把局面推进到**首个训练决策点**，返回该局面与候选动作
///
/// 搜索与决策点探针的调用点都在这里（`Train` 阶段、人头分布已完成）：直接用初始局面
/// 跑训练动作会读到尚未分配的人头分布，不是合法搜索根。推进过程本身消耗随机流，
/// `seed` 决定推进路径。
pub(crate) fn decision_root(mut game: BasicGame, seed: u64) -> Result<(BasicGame, Vec<BasicAction>)> {
    let mut rng = StdRng::seed_from_u64(seed);
    let trainer = FirstActionTrainer;
    for _ in 0..MAX_ADVANCE {
        if game.stage == BasicStage::Train {
            let actions = game.list_actions()?;
            return Ok((game, actions));
        }
        game.run_stage(&trainer, &mut rng)?;
        if !game.next() {
            bail!("推进到训练决策点之前对局已结束");
        }
    }
    bail!("推进阶段超过 {MAX_ADVANCE} 步仍未抵达训练阶段")
}

/// 用契约参考训练员的随机流推进一局到底（基线用例用）
///
/// 与 `umaai::Runner` 的推进顺序一致（`Game::run_full_game`），但不装配编排层：
/// 基线守的是**模拟数值位移**，编排另有端到端用例逐位守门。
pub(crate) fn run_full(mut game: BasicGame, seed: u64) -> Result<BasicGame> {
    let mut rng = StdRng::seed_from_u64(seed);
    game.run_full_game(&umasim::trainer::RandomTrainer, &mut rng)?;
    Ok(game)
}
