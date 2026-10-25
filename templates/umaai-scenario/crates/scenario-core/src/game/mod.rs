//! 剧本局面（`ScenarioGame`）、资源快照与装配
//!
//! 状态底座复用引擎 [`BaseGame`]（回合 / 马娘 / 卡组 / 继承 / 规则流），剧本只加自己的
//! 阶段机、人头与剧本资产。契约 impl 集中在 [`traits`]，本文件只放 inherent 方法。

pub mod action;
pub mod stage;
pub mod traits;

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use anyhow::Result;
use rand::Rng;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use umaai::Config;
use umaai_core::Data;
use umaai_core::gamedata::ActionValue;
use umaai_core::rng::{StrategyRng, StreamTag, derive_seed};
use umasim::game::base::{BaseGame, BasePerson, RuleStreams};
use umasim::game::{Game, InheritInfo, Trainer, TrainingValueSource};

pub use action::Action;
pub use stage::Stage;

use crate::data::ScenarioData;

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

/// 剧本局面
///
/// 状态底座复用引擎的 [`BaseGame`]（回合 / 马娘 / 卡组 / 继承 / 规则流），剧本只加自己的
/// 阶段机、人头、策略流与剧本资产。
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
    /// 剧本自持资产（宿主载入后注入；训练表等数值都取自它）
    pub data: Arc<ScenarioData>,
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

impl ScenarioGame {
    /// 建一局（要求宿主已初始化静态数据：`umaai_core::Data::init`）
    ///
    /// `data` 是剧本自持资产（宿主载入后注入）：训练基础值表等数值都取自它。
    pub fn newgame(
        uma_id: u32,
        deck_ids: &[u32],
        inherit: InheritInfo,
        race_grades: &[i32],
        data: Arc<ScenarioData>,
    ) -> Result<Self> {
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
            data,
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
        ScenarioView {
            sample_resource: self.data.sample_resource,
        }
    }

    /// 训练结算（模板版：按剧本资产里的表入账 + 设施计数；失败判定 / 羁绊 / hint 留给剧本作者）
    pub(crate) fn do_train(&mut self, train: usize, _rng: &mut impl Rng) -> Result<()> {
        let table = self.training_basic_table();
        let level = self.train_level(train).clamp(1, 5) - 1;
        // 模板表档位可少于 5：等级超出即取末档（真实剧本给满 5 档）
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
    pub(crate) fn reset_strategy_stream(&mut self) {
        let turn = self.base.turn as u64;
        self.strategy = self
            .base
            .master()
            .map(|master| StrategyRng::new(derive_seed(master, &[turn, StreamTag::Strategy.tag()])));
    }

    /// 单阶段推进（模板版阶段机；真实剧本按自己的回合结构改）
    pub(crate) fn stage_body<T: Trainer<Self>>(
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

/// 从引擎配置与剧本资产装配一局剧本局面（终端样例外壳与剧本侧测试共用）
///
/// 数据根与配置由宿主注入：本函数只读宿主给的 [`Config`] 与 [`ScenarioData`]，不碰任何路径。
pub fn build_game(cfg: &Config, data: Arc<ScenarioData>) -> Result<ScenarioGame> {
    ScenarioGame::newgame(cfg.sim.uma, &cfg.sim.cards, cfg.inherit(), cfg.race_grades()?, data)
}
