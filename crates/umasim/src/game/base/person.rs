//! 基础训练人头（动态信息）

use serde::{Deserialize, Serialize};
use umaai_core::Data;

use crate::game::{Person, PersonType, SupportCard};

/// 训练人头信息（动态）
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BasePerson {
    /// 人头顺序。**不保证与卡组槽位同序**（剧本友人卡可能推迟加入）。
    /// 需要由人头访问卡组时走 `Game::deck_index_of` 反查，不要拿本字段当卡组下标。
    pub person_index: i32,
    /// 人头类型
    pub person_type: PersonType,
    /// 得意训练类型，0-5:速耐力根智团 一部分npc也有 -1为没有
    pub train_type: i32,
    /// 角色ID
    pub chara_id: u32,
    /// 羁绊
    pub friendship: i32,
    /// 是否有叹号
    pub is_hint: bool,
    /// 支援卡信息
    pub card_id: Option<u32>,
}

impl BasePerson {
    /// 人头短名（NPC / 理事长 / 记者 / 卡名 / 角色名前两字）
    pub fn short_name(&self) -> String {
        let data = Data::get().expect("数据未初始化");
        match self.person_type {
            PersonType::Npc => "[NPC]".to_string(),
            PersonType::Yayoi => "理事长".to_string(),
            PersonType::Reporter => "记者".to_string(),
            _ => {
                if let Some(Ok(support)) = self.card_id.map(|id| data.get_card(id)) {
                    support.short_name().to_string()
                } else {
                    let short_chara_name: String = data.get_chara_name(self.chara_id).chars().take(2).collect();
                    format!("[???]{short_chara_name}")
                }
            }
        }
    }

    /// 理事长人头
    pub fn yayoi() -> Self {
        BasePerson {
            person_index: 6,
            person_type: PersonType::Yayoi,
            train_type: -1,
            chara_id: 9002,
            friendship: 0,
            is_hint: false,
            card_id: None,
        }
    }

    /// 记者人头
    pub fn reporter() -> Self {
        BasePerson {
            person_index: 7,
            person_type: PersonType::Reporter,
            train_type: -1,
            chara_id: 9003,
            friendship: 0,
            is_hint: false,
            card_id: None,
        }
    }
}

impl Person for BasePerson {
    fn person_type(&self) -> PersonType {
        self.person_type
    }
    fn person_index(&self) -> i32 {
        self.person_index
    }
    fn train_type(&self) -> i32 {
        self.train_type
    }
    fn friendship(&self) -> i32 {
        self.friendship
    }
    fn set_hint(&mut self, hint: bool) {
        self.is_hint = hint;
    }
    fn hint(&self) -> bool {
        self.is_hint
    }
    fn card_id(&self) -> Option<u32> {
        self.card_id
    }
}

impl TryFrom<&SupportCard> for BasePerson {
    type Error = anyhow::Error;

    /// 由支援卡构造人头（类型按卡种映射；羁绊取卡面当前值）
    fn try_from(card: &SupportCard) -> Result<Self, Self::Error> {
        let person_type = match card.card_type {
            0..=4 => PersonType::Card,
            5 => PersonType::ScenarioCard,
            6 => PersonType::TeamCard,
            _ => PersonType::Card,
        };
        Ok(BasePerson {
            person_index: 0,
            person_type,
            train_type: card.card_type,
            chara_id: card.data.chara_id,
            friendship: card.friendship,
            is_hint: false,
            card_id: Some(card.card_id),
        })
    }
}
