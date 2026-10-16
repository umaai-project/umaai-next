//! 训练员（决策侧）实现
//!
//! 当前只有契约参考实现 [`RandomTrainer`]：随机选择 + 基础优先序（体力低先睡、
//! 干劲低先出行、否则训练），供搜索 rollout 与调试使用。剧本专用策略属剧本仓。

use anyhow::Result;
use log::debug;
use rand::Rng;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use umaai_core::gamedata::EventChoice;

use crate::game::base::BaseAction;
use crate::game::{ActionEnum, Game, Trainer};

/// 随机训练员（契约参考实现）
///
/// 动作优先级：体力 `< 45` 找睡觉，干劲 `< 5` 找出行，否则找训练；都没有时取
/// 洗牌后的第一个候选。选择顺序由传入的 `rng` 决定，故同一局面同一种子可复现。
#[derive(Debug, Clone, Copy, Default)]
pub struct RandomTrainer;

impl<G: Game> Trainer<G> for RandomTrainer {
    fn select_action(&self, game: &G, actions: &[<G as Game>::Action], rng: &mut StdRng) -> Result<usize> {
        let mut order: Vec<usize> = (0..actions.len()).collect();
        order.shuffle(rng);
        let picked = order.iter().copied().find(|&i| {
            let base = actions[i].as_base_action();
            if game.uma().vital < 45 {
                base == Some(BaseAction::Sleep)
            } else if game.uma().motivation < 5 {
                matches!(base, Some(BaseAction::NormalOuting) | Some(BaseAction::FriendOuting))
            } else {
                matches!(base, Some(BaseAction::Train(_)))
            }
        });
        // 优先序全部落空（如比赛回合只剩 Race）时退化为随机候选
        let ret = picked.unwrap_or(order[0]);
        debug!("随机训练员选择动作 #{ret}（候选 {} 个）", actions.len());
        Ok(ret)
    }

    fn select_choice(&self, _game: &G, choices: &[Vec<EventChoice>], rng: &mut StdRng) -> Result<usize> {
        if choices.is_empty() {
            return Ok(0);
        }
        Ok(rng.random_range(0..choices.len()))
    }
}
