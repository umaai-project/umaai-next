//! # 空剧本仓模板（`umaai-scenario`）
//!
//! 最小可编译的剧本骨架，给出「引擎契约怎么接」的完整清单；复制到自己的剧本仓后改名
//! `umaai-<剧本>`，在 `do_train` / `generate_events` / `list_actions` 等留白处填自己的规则。
//!
//! ## 纪律（详见 `docs/scenario/README.md` 与 `docs/scenario/contracts.md`）
//!
//! - **阶段自持**：内部用强类型枚举，对外只投影 [`StageId`]（显式赋号，改号等于旧种子失效）。
//! - **三条随机轴**：局面流 / 事件流从 [`BaseGame`] 取出（`take_rule_streams`），策略流本局面
//!   自持，**决策流只给 `Trainer`**；每进入新回合三条轴一起按 `(master, 回合)` 重新派生。
//! - **注入点**：`set_rule_master` 装主种子并派生各轴，未注入就推进会报错（不静默回退）。
//! - **数据与配置由宿主注入**：本 crate 不写死路径；引擎静态表走 `umaai_core::Data`。
//! - **搜索接入**：实现 [`FlatSearchGame`]，分支只能经 `fork_for_rollout` 建（克隆 + 注入 rollout 种子）。

use std::fmt::Display;
use std::ops::{Deref, DerefMut};
use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use rand::Rng;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use umaai::Config;
use umaai_core::Data;
use umaai_core::gamedata::{ActionValue, EventData};
use umaai_core::rng::{StrategyRng, StreamTag, derive_seed};
use umasim::game::base::{BaseGame, BasePerson, RuleStreams};
use umasim::game::{
    ActionEnum, Game, InheritInfo, StageId, SupportCard, Trainer, TrainingBasicTable, TrainingValueSource, Uma,
};
use umasim::search::FlatSearchGame;
use umasim::trainer::RandomTrainer;

/// 育成最大回合（0 基，78 回合）；真实剧本按自己的赛程改
pub const MAX_TURN: i32 = 77;

/// 训练基础值表：`[训练 0-4][等级 0-4] -> [速 耐 力 根 智 PT 体力消耗]`
///
/// 模板只给一张可编译的示例表；真实剧本换成自己的数值表（数值是剧本资产）。
static TRAINING_TABLE: OnceLock<TrainingBasicTable> = OnceLock::new();

/// 模板训练表（惰性初始化）
fn training_table() -> &'static TrainingBasicTable {
    TRAINING_TABLE.get_or_init(|| {
        vec![
            vec![[12, 0, 2, 0, 0, 7, -20], [14, 0, 3, 0, 0, 7, -25]],
            vec![[0, 11, 0, 3, 0, 7, -22], [0, 14, 0, 4, 0, 7, -27]],
            vec![[0, 3, 11, 0, 0, 7, -22], [0, 4, 14, 0, 0, 7, -27]],
            vec![[2, 0, 2, 12, 0, 7, -22], [2, 0, 2, 15, 0, 7, -27]],
            vec![[2, 0, 0, 0, 11, 7, 5], [2, 0, 0, 0, 12, 7, 5]],
        ]
    })
}

/// 剧本阶段（内部强类型；对外只投影 [`StageId`]）
///
/// 纪律：**无数据、无引用、可 `Copy`**；中间状态（吃了哪碗面 / 选了哪个地区）放局面字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stage {
    /// 回合开始（事件链）
    #[default]
    Begin,
    /// 训练 / 比赛决策点
    Train,
    /// 回合结算
    End,
}

impl Stage {
    /// 下一个阶段（`None` = 本回合走完）
    fn next(self) -> Option<Self> {
        match self {
            Self::Begin => Some(Self::Train),
            Self::Train => Some(Self::End),
            Self::End => None,
        }
    }
}

impl From<Stage> for StageId {
    /// 显式赋号：**号是稳定契约**，改号等于旧种子与基线失效
    fn from(stage: Stage) -> Self {
        Self::new(match stage {
            Stage::Begin => 0,
            Stage::Train => 1,
            Stage::End => 2,
        })
    }
}

/// 剧本动作
///
/// `apply` 只负责把动作落到局面上，**取哪条随机流由 `run_stage` 决定**（策略流）。
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// 训练（0-4：速 / 耐 / 力 / 根 / 智）
    Train(u8),
    /// 休息
    Rest,
}

impl Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Train(idx) => write!(f, "训练{idx}"),
            Self::Rest => write!(f, "休息"),
        }
    }
}

impl ActionEnum for Action {
    type Game = ScenarioGame;

    fn apply(&self, game: &mut Self::Game, rng: &mut impl Rng) -> Result<()> {
        match self {
            Self::Train(idx) => game.do_train(*idx as usize, rng),
            Self::Rest => {
                // 模板留白：休息恢复体力（真实剧本按自己的回复规则改）
                let _ = rng;
                let uma = game.uma_mut();
                uma.vital = (uma.vital + 20).min(uma.max_vital);
                Ok(())
            }
        }
    }
}

/// 剧本局面
///
/// 状态底座复用引擎的 [`BaseGame`]（回合 / 马娘 / 卡组 / 继承 / 规则流），剧本只加自己的
/// 阶段机、人头与资源字段。
#[derive(Debug, Clone, PartialEq)]
pub struct ScenarioGame {
    /// 引擎状态底座（经 `Deref` 取用；底座方法名不得与契约方法同名，见 AGENTS.md）
    pub base: BaseGame,
    /// 人头（模板直接复用引擎参考实现 `BasePerson`；真实剧本可定义自己的 `Person`）
    pub persons: Vec<BasePerson>,
    /// 剧本自持阶段机
    pub stage: Stage,
    /// 剧本自持策略流（动作结算与剧本机制随机；从规则主种子派生）
    pub strategy: Option<StrategyRng>,
}

impl Deref for ScenarioGame {
    type Target = BaseGame;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for ScenarioGame {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

/// 剧本资源快照（给前端的纯数据 View）
///
/// 形态 A 五条纪律（见 `docs/scenario/contracts.md`）：**单一取数口** / 只放引擎不知道的字段 /
/// 快照语义（幂等、无副作用）/ 可演进（`non_exhaustive`）/ **不进契约 trait**。
/// 模板留一个占位字段，真实剧本按自己的资源（PT / 地区 / 挖掘…）增删。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ScenarioView {
    /// 示例：剧本自持资源（模板占位，接上真实资源后改取数）
    pub sample_resource: i32,
}

impl ScenarioGame {
    /// 建一局（要求宿主已初始化静态数据：`umaai_core::Data::init`）
    pub fn newgame(uma_id: u32, deck_ids: &[u32], inherit: InheritInfo, race_grades: &[i32]) -> Result<Self> {
        let mut game = Self {
            base: BaseGame::new(
                uma_id,
                deck_ids,
                inherit,
                Data::get()?.consts.five_status_limit_base,
                race_grades,
            )?,
            persons: Vec::new(),
            stage: Stage::Begin,
            strategy: None,
        };
        game.init_persons()?;
        Ok(game)
    }

    /// 追加一个人头（按当前人数分配人头序号）
    pub fn add_person(&mut self, mut person: BasePerson) {
        person.person_index = self.persons.len() as i32;
        self.persons.push(person);
    }

    /// 取剧本资源快照（**单一取数口**；何时取由宿主决定）
    #[must_use]
    pub fn view(&self) -> ScenarioView {
        ScenarioView { sample_resource: 0 }
    }

    /// 训练结算（模板版：按表入账 + 设施计数；失败判定 / 羁绊 / hint 留给剧本作者）
    fn do_train(&mut self, train: usize, _rng: &mut impl Rng) -> Result<()> {
        let table = self.training_basic_table();
        let level = self.train_level(train).clamp(1, 5) - 1;
        // 模板表只给 2 档：等级超出即取末档（真实剧本给满 5 档）
        let row = table
            .get(train)
            .and_then(|levels| levels.get(level).or_else(|| levels.last()))
            .copied()
            .unwrap_or([0; 7]);
        let value = ActionValue {
            status_pt: [row[0], row[1], row[2], row[3], row[4], row[5]],
            vital: row[6],
            ..Default::default()
        };
        self.uma_mut().add_value(&value);
        self.base.train_level_count[train] += 1;
        Ok(())
    }

    /// 派生本局面自持的策略流（注入点与回合边界调用）
    fn reset_strategy_stream(&mut self) {
        let turn = self.base.turn as u64;
        self.strategy = self
            .base
            .master()
            .map(|master| StrategyRng::new(derive_seed(master, &[turn, StreamTag::Strategy.tag()])));
    }

    /// 单阶段推进（模板版阶段机；真实剧本按自己的回合结构改）
    fn stage_body<T: Trainer<Self>>(
        &mut self,
        trainer: &T,
        decision: &mut StdRng,
        streams: &mut RuleStreams,
        strategy: &mut StrategyRng,
    ) -> Result<()> {
        match self.stage {
            Stage::Begin => {
                // 模板留白：回合开始的事件链。真实剧本在这里用 `streams.event`（事件流）生成
                // 并结算回合事件、用 `streams.turn_fixed`（回合固定流）做人头分配 / 角标 / hint。
                let _ = (&mut streams.event, &mut streams.turn_fixed);
            }
            Stage::Train => {
                let actions = self.list_actions()?;
                let selection = trainer.select_action(self, &actions, decision)?;
                // 动作结算走**策略流**：与决策抽签分开，改决策流消耗不会位移结算随机
                self.apply_action(&actions[selection], strategy)?;
            }
            Stage::End => {}
        }
        Ok(())
    }
}

impl TrainingValueSource for ScenarioGame {
    /// 模板表；真实剧本换成自己的数值表
    fn training_basic_table(&self) -> &TrainingBasicTable {
        training_table()
    }
}

impl Game for ScenarioGame {
    type Person = BasePerson;
    type Action = Action;

    fn turn(&self) -> i32 {
        self.base.turn
    }

    fn max_turn(&self) -> i32 {
        MAX_TURN
    }

    fn rule_master(&self) -> Option<u64> {
        self.base.master()
    }

    fn set_rule_master(&mut self, master: u64) {
        // 引擎两轴 + 剧本自持的策略轴一起重置（底座入口叫 init_rule_streams，刻意不与契约同名）
        self.base.init_rule_streams(master);
        self.reset_strategy_stream();
    }

    fn stage(&self) -> StageId {
        self.stage.into()
    }

    fn next(&mut self) -> bool {
        if let Some(stage) = self.stage.next() {
            self.stage = stage;
        } else if self.turn < self.max_turn() {
            self.turn += 1;
            self.stage = Stage::Begin;
            // 新回合：两条引擎流与自持的策略流一起按 `(master, 新回合)` 重新派生（CRN 配对来源）
            self.base.reset_rule_streams();
            self.reset_strategy_stream();
        } else {
            return false;
        }
        true
    }

    fn run_stage<T: Trainer<Self>>(&mut self, trainer: &T, decision: &mut StdRng) -> Result<()> {
        // 三条轴各就各位：局面流 / 事件流从底座取出，策略流本局面自持，决策流只给 Trainer。
        // 取出-放回成对执行（中途出错也先放回再传播），避免推进过的流状态被丢掉。
        let mut streams = self.base.take_rule_streams()?;
        let Some(mut strategy) = self.strategy else {
            return Err(anyhow!(
                "策略流缺失：主种子已注入但策略流未派生，应先调用 set_rule_master"
            ));
        };
        let res = self.stage_body(trainer, decision, &mut streams, &mut strategy);
        self.base.put_rule_streams(streams);
        self.strategy = Some(strategy);
        res
    }

    fn list_actions(&self) -> Result<Vec<Self::Action>> {
        // 模板：五个训练 + 休息；真实剧本在这里列自己的动作（比赛 / 出行 / 剧本机制）
        let mut actions: Vec<Action> = (0..5u8).map(Action::Train).collect();
        actions.push(Action::Rest);
        Ok(actions)
    }

    fn generate_events(&self, _rng: &mut impl Rng) -> Vec<EventData> {
        // 模板留白：真实剧本在这里生成回合事件（固定回合 / 概率 / 剧本条件）
        Vec::new()
    }

    fn apply_event(&mut self, event: &EventData, choice: usize, rng: &mut impl Rng) -> Result<()> {
        // 引擎底座负责通用事件结算；剧本自己的特殊事件在这里补分支
        let _ = self.base.apply_event(event, choice, rng);
        Ok(())
    }

    fn persons(&self) -> &[Self::Person] {
        &self.persons
    }

    fn persons_mut(&mut self) -> &mut [Self::Person] {
        &mut self.persons
    }

    fn init_persons(&mut self) -> Result<()> {
        // 模板：卡组人头 + 理事长；真实剧本按自己的卡池与友人规则构造
        let persons: Vec<BasePerson> = self
            .base
            .deck
            .iter()
            .map(BasePerson::try_from)
            .collect::<Result<Vec<_>>>()?;
        for person in persons {
            self.add_person(person);
        }
        self.add_person(BasePerson::yayoi());
        Ok(())
    }

    fn distribution(&self) -> &Vec<Vec<i32>> {
        &self.base.distribution
    }

    fn distribution_mut(&mut self) -> &mut Vec<Vec<i32>> {
        &mut self.base.distribution
    }

    fn absent_rate_drop(&self) -> i32 {
        self.base.absent_rate_drop
    }

    fn deyilv(&mut self, _person_index: i32) -> f32 {
        // 模板留白：真实剧本按自己的卡组效果算「人头得意率」
        1.0
    }

    fn has_group_buff(&self) -> bool {
        self.base.friend.group_buff_turn > 0
    }

    fn train_level(&self, train: usize) -> usize {
        (self.base.train_level_count[train] as usize / 4 + 1).clamp(1, 5)
    }

    fn uma(&self) -> &Uma {
        &self.base.uma
    }

    fn uma_mut(&mut self) -> &mut Uma {
        &mut self.base.uma
    }

    fn deck(&self) -> &Vec<SupportCard> {
        &self.base.deck
    }
}

impl FlatSearchGame for ScenarioGame {
    /// rollout 基策（模板用契约参考实现；真实剧本换成自己的策略）
    type RolloutTrainer = RandomTrainer;

    /// 没有 leaf 估值器时声明 `false`：搜索入口会直接拒绝 `max_depth > 0`
    const SUPPORTS_TRUNCATED_LEAF: bool = false;

    fn default_rollout_trainer() -> Self::RolloutTrainer {
        RandomTrainer
    }

    fn apply_root_action(&mut self, action: &Self::Action, rng: &mut StdRng) -> Result<()> {
        // 必须与 `run_stage` 内执行动作的方式逐字一致，否则被评估的动作会用上另一条流
        self.apply_action(action, rng)
    }

    fn fork_for_rollout(&self, rollout_seed: u64) -> Self {
        // 分支 = 克隆 + 注入 rollout 主种子；任何不由传入 rng 驱动的流都必须在这里重置
        let mut game = self.clone();
        game.set_rule_master(rollout_seed);
        game
    }
}

/// 从引擎配置装配一局剧本局面（终端样例外壳与剧本侧测试共用）
///
/// 数据根与配置由宿主注入：本函数只读宿主给的 [`Config`]，不碰任何路径。
pub fn build_game(cfg: &Config) -> Result<ScenarioGame> {
    ScenarioGame::newgame(cfg.sim.uma, &cfg.sim.cards, cfg.inherit(), cfg.race_grades()?)
}
