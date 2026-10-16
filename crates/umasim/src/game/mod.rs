//! 游戏通用枚举与状态（剧本无关）
//!
//! 回合阶段 / 训练人头类型 / 友人状态系列，被各剧本 `Game` 实现共用；
//! 具体剧本类型不在此层（见方案 v2 §3 层界）。

pub mod base;
pub mod inherit;
pub mod support_card;
pub mod traits;
pub mod uma;

use anyhow::{Result, anyhow};
use enum_iterator::Sequence;
use serde::{Deserialize, Serialize};
use umaai_core::Data;
use umaai_core::gamedata::{ActionValue, ChoiceResult, EventChoice, EventData};

pub use base::*;
pub use inherit::InheritInfo;
pub use support_card::*;
pub use traits::*;
pub use uma::*;

/// 回合阶段
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Sequence)]
pub enum TurnStage {
    /// 1. 回合开始
    #[default]
    Begin,
    /// 2. 分配人头前，随机事件
    Distribute,
    // --- 可操作部分
    /// 3. 选择剧本buff前
    StoryShop,
    /// 4. 选择训练或比赛
    Train,
    /// 5. 回合后事件
    AfterTrain,
    // ---
    /// 6. 结束，剧本结算
    End,
}

/// 训练人头类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PersonType {
    /// 携带的支援卡
    #[default]
    Card,
    /// 剧本友人
    ScenarioCard,
    /// NPC
    Npc,
    /// 理事长
    Yayoi,
    /// 记者
    Reporter,
    /// 其他友人
    OtherFriend,
    /// 团队卡
    TeamCard,
}

/// 友人卡档
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FriendCardState {
    /// 没带
    #[default]
    Empty,
    /// 3星友人
    SSR,
    /// 1星友人
    R,
}

/// 友人出行阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FriendOutState {
    /// 未点击
    #[default]
    UnClicked,
    /// 已点击，未出行
    BeforeUnlock,
    /// 已出行
    AfterUnlock,
    /// 离开
    Away,
}

/// 友人出行信息，剧本通用
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FriendState {
    /// 剧本友人携带状态
    pub card_state: FriendCardState,
    /// 友人出行阶段
    pub out_state: FriendOutState,
    /// 友人的出行用了哪几段
    pub out_used: Vec<bool>,
    /// 团队卡Buff已经持续几回合，0为未开启
    pub group_buff_turn: u32,
    /// 友人在Persons中是哪一个
    pub person_index: usize,
    /// 友人事件体力回复量加成
    pub vital_bonus: i32,
    /// 友人事件效果加成
    pub event_bonus: i32,
}

/// 红点属性事件（数值取自常量表；构造归本层）
pub fn hint_attr_event(train: usize, person_index: usize) -> Result<EventData> {
    let cons = &Data::get()?.consts;
    if train >= 5 {
        return Err(anyhow!("train越界: {train}"));
    }
    let train_name = &cons.train_names[train];
    let value = ActionValue {
        status_pt: cons.hint_event_value[train],
        friendship: 5,
        ..Default::default()
    };
    let choice = EventChoice::from_action_value(&value, ChoiceResult::Normal);
    Ok(EventData {
        id: 101,
        name: format!("Hint - {train_name}属性"),
        person_index: Some(person_index as i32),
        choices: vec![vec![choice]],
        ..Default::default()
    })
}

/// 红点技能事件
pub fn hint_skill_event(hint_level: i32, person_index: usize) -> EventData {
    let value = ActionValue {
        hint_level,
        friendship: 5,
        ..Default::default()
    };
    let choice = EventChoice::from_action_value(&value, ChoiceResult::Normal);
    EventData {
        id: 101,
        name: "Hint - 技能".to_string(),
        person_index: Some(person_index as i32),
        choices: vec![vec![choice]],
        ..Default::default()
    }
}

/// 加练事件（系统事件模板 + 按训练位写属性）
pub fn extra_training_event(train: usize) -> Result<EventData> {
    let mut ret = Data::get()?.system_event("extra_train")?.clone();
    ret.choices[0][0].value.status_pt[train] = 5;
    Ok(ret)
}

impl FriendState {
    /// 由友人卡的 `idrank`（`id * 10 + rank`）构造出行状态
    ///
    /// `idrank` 为 `None` 表示没带友人卡。`rank > 4` 视为非法输入。
    pub fn new(idrank: Option<u32>, index: usize) -> Result<Self> {
        let data = Data::get()?;
        let mut ret = FriendState::default();
        ret.out_used = vec![false, false, false, false, false];
        if let Some(i) = idrank {
            let (id, rank) = (i / 10, i % 10);
            if rank > 4 {
                return Err(anyhow!("idrank {i} 非法"));
            }
            let card = data.get_card(id)?;
            ret.card_state = match card.rarity {
                3 => FriendCardState::SSR,
                _ => FriendCardState::R,
            };
            ret.vital_bonus = card.card_value[rank as usize].event_recovery_amount_up;
            ret.event_bonus = card.card_value[rank as usize].event_effect_up;
        }
        ret.person_index = index;
        Ok(ret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umaai_core::testing::data_root;

    /// 层1：友人卡构造（SSR 档 + 加成字段取自 cardDB）
    #[test]
    fn test_friend() -> Result<()> {
        Data::init(data_root()?)?;
        let friend = FriendState::new(Some(302574), 3)?;
        // 302574 → id 30257, rank 4；rarity 3 → SSR
        assert_eq!(friend.card_state, FriendCardState::SSR);
        assert_eq!(friend.person_index, 3);
        assert_eq!(friend.out_used.len(), 5);
        // rank 4 的加成字段取自卡面（非零才说明真的读到了表）
        println!("friend: {friend:?}");
        Ok(())
    }
}
