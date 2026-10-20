//! 基础动作（`BaseAction`）：对 [`BaseGame`] 的通用变换

use std::fmt::Display;

use anyhow::{Result, anyhow};
use rand::Rng;
use serde::{Deserialize, Serialize};
use umaai_core::gamedata::ActionValue;
use umaai_core::{Data, Ill};

use super::BaseGame;
use crate::game::ActionEnum;

/// 基础动作枚举（剧本动作的公共子集）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BaseAction {
    /// 训练
    Train(i32),
    /// 比赛
    Race,
    /// 休息
    Sleep,
    /// 友人出行
    FriendOuting,
    /// 普通出行
    NormalOuting,
    /// 治病
    Clinic,
}

impl Display for BaseAction {
    /// 稳定标识（不读静态数据；人读文案由呈现层映射）
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BaseAction::Train(x) => write!(f, "Train({x})"),
            BaseAction::Race => write!(f, "Race"),
            BaseAction::Sleep => write!(f, "Sleep"),
            BaseAction::FriendOuting => write!(f, "FriendOuting"),
            BaseAction::NormalOuting => write!(f, "NormalOuting"),
            BaseAction::Clinic => write!(f, "Clinic"),
        }
    }
}

// 具体的操作都是 &mut BaseGame -> Result<()> 的映射。
// 在具体剧本类型中可以直接调用 BaseAction 的方法得到 BaseGame 部分的变换；
// 但 do_train 需要具体类型自己实现，因为 BaseGame 没有实现 Game trait，
// 不能使用 Game trait 里计算训练数值的方法。
impl BaseAction {
    /// 比赛动作（生涯 / 自选；事件面板乘算比赛加成）
    ///
    /// 事件构造与缩放统一走 [`BaseGame::push_race_event`]：默认系数即
    /// `(100 + race_bonus) / 100`，剧本可用同一入口传自己的实际系数。
    pub fn do_race(game: &mut BaseGame, _rng: &mut impl Rng) -> Result<()> {
        let scale = (100 + game.uma.race_bonus) as f64 / 100.0;
        game.push_race_event(scale, 1.0)
    }

    /// 休息动作（小合宿固定 +40 体 +1 干劲；平时按分布抽 +30/+50/+70）
    pub fn do_sleep(game: &mut BaseGame, rng: &mut impl Rng) -> Result<()> {
        if game.is_xiahesu() {
            let value = ActionValue {
                vital: 40,
                motivation: 1,
                ..Default::default()
            };
            game.uma.add_value(&value);
        } else {
            // 热路径零堆分配分桶（与 WeightedIndex 抽样序列逐位一致）
            let weights = &Data::get()?.consts.rest_probs;
            let mut value = ActionValue::default();
            match crate::game::sample_bucket(weights, rng)? {
                0 => {
                    value.vital = 30;
                    game.uma.add_value(&value);
                }
                1 => {
                    value.vital = 50;
                    game.uma.add_value(&value);
                }
                _ => {
                    value.vital = 70;
                    game.uma.add_value(&value);
                }
            }
        }
        Ok(())
    }

    /// 友人出行动作（按段依次使用；事件放到回合后经 Trainer 选择执行）
    pub fn do_friend_outing(game: &mut BaseGame, _rng: &mut impl Rng) -> Result<()> {
        let mut which = 0;
        while which < 5 && game.friend.out_used[which] {
            which += 1;
        }
        if which < 5 {
            // 归属在结算时按友人身份定位（`BasicGame::apply_event`）：动作层拿不到人头容器
            let event = Data::get()?.events.friend_events[&(1 + which).to_string()].clone();
            game.friend.out_used[which] = true;
            game.unresolved_events.push(event);
            Ok(())
        } else {
            Err(anyhow!("友人出行越界: {which}"))
        }
    }

    /// 普通出行动作（随机 1-3 档事件；24 回合后可顺带抓娃娃）
    pub fn do_normal_outing(game: &mut BaseGame, rng: &mut impl Rng) -> Result<()> {
        let event_name = format!("normal_outing_{}", rng.random_range(1..=3));
        game.unresolved_events
            .push(Data::get()?.system_event(&event_name)?.clone());
        // 抓娃娃
        if game.turn >= 24 && !game.uma.flags.doll && rng.random_bool(Data::get()?.event_prob("doll")?) {
            game.unresolved_events
                .push(Data::get()?.system_event("normal_outing_doll")?.clone());
            game.uma.flags.doll = true;
        }

        Ok(())
    }

    /// 治病动作（+20 体；清空生病位集与不擅长训练）
    pub fn do_clinic(game: &mut BaseGame, _rng: &mut impl Rng) -> Result<()> {
        let value = ActionValue {
            vital: 20,
            ..Default::default()
        };
        game.uma.flags.ill = Ill::NONE;
        game.uma.flags.bad_trainer = false;
        game.uma.add_value(&value);
        Ok(())
    }
}

/// 实现基础动作对基础游戏状态的变换，作为实际剧本动作的一部分
impl ActionEnum for BaseAction {
    type Game = BaseGame;

    fn apply(&self, game: &mut Self::Game, rng: &mut impl Rng) -> Result<()> {
        match self {
            BaseAction::Train(_) => Err(anyhow!("BaseGame没有实现训练，需要在具体类型中重新实现")),
            BaseAction::Race => BaseAction::do_race(game, rng),
            BaseAction::Sleep => BaseAction::do_sleep(game, rng),
            BaseAction::FriendOuting => BaseAction::do_friend_outing(game, rng),
            BaseAction::NormalOuting => BaseAction::do_normal_outing(game, rng),
            BaseAction::Clinic => BaseAction::do_clinic(game, rng),
        }
    }

    fn as_base_action(&self) -> Option<BaseAction> {
        Some(self.clone())
    }
}

/// 用于保存动作和得分（搜索 / 决策层的通用产物）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActionScore<A: ActionEnum + Serialize> {
    /// 选中的动作
    pub selection: A,
    /// 候选动作列表
    pub actions: Vec<A>,
    /// 各候选得分
    pub score: Vec<f64>,
}

impl<A: ActionEnum + Serialize> ActionScore<A> {
    /// 构造（无得分时补零对齐候选长度）
    pub fn new(selection: A, actions: Vec<A>, score: Vec<f64>) -> Self {
        if score.is_empty() && !actions.is_empty() {
            let default_score = vec![0.0; actions.len()];
            Self {
                selection,
                actions,
                score: default_score,
            }
        } else {
            Self { selection, actions, score }
        }
    }
}
