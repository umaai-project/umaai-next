//! 局中支援卡（领域状态），剧本通用

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use umaai_core::gamedata::{CardValue, SupportCardData};
use umaai_core::{Array6, AttributeArray, Data};

use crate::game::Game;

/// 局中带入计算的支援卡属性。其他不变的属性从数据表里取
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CardTrainingEffect {
    /// 友情
    pub youqing: f32,
    /// 干劲
    pub ganjing: i32,
    /// 训练
    pub xunlian: i32,
    /// 赛后
    pub saihou: i32,
    /// 得意率
    pub deyilv: f32,
    /// 副属性
    pub bonus: Array6,
    /// 智训练体力恢复
    pub wiz_vital_bonus: i32,
    /// 失败率下降
    pub fail_rate_drop: f32,
    /// 体力消费下降
    pub vital_cost_drop: f32,
    /// 事件效果提高，乘算
    pub event_effect_up: i32,
    /// 事件回复量提高，乘算
    pub event_recovery_amount_up: i32,
    /// Hint数量(由固有改变)
    pub hint_count_bonus: i32,
}

impl CardTrainingEffect {
    /// 根据公式与另一个效果叠加，不包含得意率
    pub fn add(&self, other: &CardTrainingEffect) -> Self {
        let mut new_bonus = self.bonus;
        new_bonus.add_eq(&other.bonus);
        Self {
            youqing: (100.0 + self.youqing) * (100.0 + other.youqing) / 100.0 - 100.0,
            ganjing: self.ganjing + other.ganjing,
            xunlian: self.xunlian + other.xunlian,
            saihou: self.saihou + other.saihou,
            deyilv: self.deyilv, // 不计算
            bonus: new_bonus,
            wiz_vital_bonus: self.wiz_vital_bonus + other.wiz_vital_bonus,
            fail_rate_drop: 100.0 - (100.0 - self.fail_rate_drop) * (100.0 - other.fail_rate_drop) / 100.0,
            vital_cost_drop: 100.0 - (100.0 - self.vital_cost_drop) * (100.0 - other.vital_cost_drop) / 100.0,
            event_effect_up: self.event_effect_up + other.event_effect_up,
            event_recovery_amount_up: self.event_recovery_amount_up + other.event_recovery_amount_up,
            hint_count_bonus: self.hint_count_bonus, // 不叠加，单独计算
        }
    }

    /// 修改单个词条（effect_id → 词条类型）
    pub fn add_effect_line(&mut self, effect_id: i32, value: i32) -> &mut Self {
        match effect_id {
            1 => {
                // 友情
                self.youqing = (100.0 + self.youqing) * (100.0 + value as f32) / 100.0 - 100.0;
            }
            2 => {
                // 干劲
                self.ganjing += value;
            }
            3..8 => {
                // 副属性
                self.bonus[(effect_id - 3) as usize] += value;
            }
            8 => {
                // 训练
                self.xunlian += value;
            }
            9..15 => {}
            15 => {
                // 赛后
                self.saihou += value;
            }
            19 => {
                // 得意率
                self.deyilv = (100.0 + self.deyilv) * (100.0 + value as f32) / 100.0 - 100.0;
            }
            27 => {
                // 失败率减乘
                self.fail_rate_drop = 100.0 - (100.0 - self.fail_rate_drop) * (100.0 - value as f32) / 100.0;
            }
            28 => {
                // 耗体减乘
                self.vital_cost_drop = 100.0 - (100.0 - self.vital_cost_drop) * (100.0 - value as f32) / 100.0;
            }
            30 => {
                // pt加成
                self.bonus[5] += value;
            }
            31 => {
                // 智力回体
                self.wiz_vital_bonus += value;
            }
            33 => {
                // Hint数量增加
                self.hint_count_bonus += value;
            }
            41 => {
                // ALL
                for i in 0..5 {
                    self.bonus[i] += 1;
                }
            }
            _ => {
                // 未知词条：数据错误，静默跳过由上层校验兜底
            }
        }
        self
    }
}

impl From<&CardValue> for CardTrainingEffect {
    fn from(v: &CardValue) -> Self {
        Self {
            youqing: v.youqing,
            ganjing: v.ganjing,
            xunlian: v.xunlian,
            saihou: v.saihou,
            deyilv: v.deyilv,
            bonus: v.bonus,
            wiz_vital_bonus: v.wiz_vital_bonus,
            fail_rate_drop: v.fail_rate_drop,
            vital_cost_drop: v.vital_cost_drop,
            event_effect_up: v.event_effect_up,
            event_recovery_amount_up: v.event_recovery_amount_up,
            hint_count_bonus: 0, // 面板中没有此词条
        }
    }
}

/// 局中的支援卡信息，剧本通用
#[derive(Debug, Clone, PartialEq)]
pub struct SupportCard {
    /// 直接借用进程期只读全局卡表中的面板数据
    pub data: &'static SupportCardData,
    /// 支援卡ID(5位)
    pub card_id: u32,
    /// 突破等级
    pub rank: u32,
    /// 支援卡类型
    pub card_type: i32,
    /// 羁绊
    pub friendship: i32,
    /// 是否剧本链接
    pub is_link_card: bool,
    /// 属性是否不会再更新
    pub is_locked: bool,
    /// 支援卡基础属性
    pub effect: CardTrainingEffect,
    /// 固有状态
    pub effect_state: HashMap<String, u32>,
    /// 已经获得的Hint等级
    pub total_hints: i32,
}

impl SupportCard {
    /// 短名（含突破等级）
    pub fn short_name(&self) -> String {
        format!("{}◆ {}", self.data.short_name(), self.rank)
    }

    /// 当前突破等级的面板数值；高频调用路径允许 panic
    #[inline]
    pub fn card_value(&self) -> &CardValue {
        &self.data.card_value[self.rank as usize]
    }

    /// 初始属性加成
    pub fn initial_bonus(&self) -> &Array6 {
        &self.card_value().initial_bonus
    }

    /// 由 `idrank`（卡ID + 突破等级，形如 301614）构造
    pub fn new(idrank: u32) -> Result<Self> {
        let (id, rank) = (idrank / 10, idrank % 10);
        let data = Data::get()?.get_card(id)?;
        if rank > 4 {
            return Err(anyhow!("Rank超出范围: {idrank}"));
        }
        let effect = CardTrainingEffect::from(&data.card_value[rank as usize]);
        let friendship = data.card_value[rank as usize].initial_jiban;
        Ok(Self {
            data,
            card_id: id,
            rank,
            card_type: data.card_type,
            effect,
            friendship,
            is_link_card: false,
            is_locked: false,
            effect_state: HashMap::new(),
            total_hints: 0,
        })
    }

    /// 计算支援卡当前 buff（基础面板 + 已触发的 unique_effect 词条）
    ///
    /// 起点是基础面板，确保每次返回 fresh cumulative，
    /// 不会被 Game impl override 的 lock 回写后多次累加 unique_effect 词条。
    pub fn calc_training_effect<G: Game>(&self, game: &G, train: i32) -> CardTrainingEffect {
        let mut ret = CardTrainingEffect::from(&self.data.card_value[self.rank as usize]);
        let param = &self.data.unique_effect_param;
        match self.data.unique_effect_type {
            0 => {}
            1 | 2 => {
                // 羁绊>args[1]时触发词条args[2] = args[3]
                if self.friendship >= param[1] {
                    if param[2] > 0 {
                        ret.add_effect_line(param[2], param[3]);
                    }
                    if param[4] > 0 {
                        ret.add_effect_line(param[4], param[5]);
                    }
                }
            }
            13 => {
                // b95 参与友情训练时触发
                if game.shining_count(train as usize) > 0 {
                    ret.add_effect_line(param[1], param[2]);
                }
            }
            21 => {
                // 编成卡种数 >= param[1] 时触发词条 param[2] = param[3]
                // （103 型经导出脚本特判重排，param[2] 恒为 8 = 训练词条）
                let mut seen = [false; 7];
                for c in game.deck() {
                    if (0..7).contains(&c.card_type) {
                        seen[c.card_type as usize] = true;
                    }
                }
                let kinds = seen.iter().filter(|&&s| s).count() as i32;
                if kinds >= param[1] {
                    ret.add_effect_line(param[2], param[3]);
                }
            }
            20
                // 巨匠: 羁绊>80时根据卡组决定加成
                if self.friendship >= param[2] => {
                    let mut card_type_count = [0; 6];
                    for c in game.deck() {
                        if c.card_type >= 5 {
                            card_type_count[5] += 1;
                        } else {
                            card_type_count[c.card_type as usize] += 1;
                        }
                    }
                    for i in 0..5 {
                        if card_type_count[i] > 0 {
                            // 0-4对应副属性词条#3-7
                            ret.add_effect_line((i + 3) as i32, card_type_count[i].max(2));
                        }
                    }
                    if card_type_count[5] > 0 {
                        ret.add_effect_line(30, card_type_count[5].max(2));
                    }
                }
            _ => {
                // 未实现的固有逻辑类型
            }
        } // match unique_effect_type
        ret
    }
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;
    use crate::game::InheritInfo;
    use crate::game::base::basic::BasicGame;
    use crate::testutil::RACE_GRADES;
    use umaai_core::testing::data_root;

    /// 不同突破等级共享全局面板，克隆后的动态卡状态独立
    #[test]
    fn test_support() -> Result<()> {
        Data::init(data_root()?)?;
        let mut card = SupportCard::new(302424)?;
        println!("card: {card:?}");
        let card2 = SupportCard::new(302464)?;
        println!("card2: {:?}", card2.effect.add(&card.effect));
        let panel = Data::get()?.get_card(card.card_id)?;
        let original_value = card.card_value().clone();
        let rank0 = SupportCard::new(302420)?;
        // 同卡不同突破等级直接共享全局面板
        assert!(ptr::eq(card.data, panel) && ptr::eq(rank0.data, panel));
        // 构造另一突破等级不改变原卡数值；各卡读取自身突破等级的数值
        assert!(card.card_value() == &original_value);
        assert!(card.card_value() == &panel.card_value[4] && rank0.card_value() == &panel.card_value[0]);

        card.effect_state.insert("test".to_string(), 1);
        let original_effect = card.effect.clone();
        let original_friendship = card.friendship;
        let mut branch = card.clone();
        branch.effect_state.insert("test".to_string(), 2);
        branch.effect.xunlian += 1;
        branch.friendship += 1;
        println!(
            "克隆动态状态: {:?}，训练效果: {:?}，羁绊: {}",
            branch.effect_state, branch.effect, branch.friendship
        );
        // 克隆继续共享同一全局面板；修改克隆不影响原卡
        assert!(ptr::eq(branch.data, panel));
        assert!(card.effect_state.get("test") == Some(&1));
        assert!(card.effect == original_effect && card.friendship == original_friendship);
        assert!(card.card_value() == &original_value && branch.card_value() == &original_value);
        Ok(())
    }

    /// 副属性叠加（add_eq）与乘区叠加公式
    #[test]
    fn test_effect_add() {
        let mut a = CardTrainingEffect::default();
        a.youqing = 20.0;
        a.ganjing = 10;
        a.bonus = [1, 2, 3, 4, 5, 6];
        let b = CardTrainingEffect {
            youqing: 30.0,
            ganjing: 5,
            bonus: [10, 0, 0, 0, 0, 1],
            ..Default::default()
        };
        let c = a.add(&b);
        assert_eq!(c.youqing, (120.0 * 130.0 / 100.0) - 100.0);
        assert_eq!(c.ganjing, 15);
        assert_eq!(c.bonus, [11, 2, 3, 4, 5, 7]);

        // add_effect_line：友情乘区与未知词条
        let mut e = CardTrainingEffect::default();
        e.add_effect_line(1, 50);
        assert_eq!(e.youqing, 50.0);
        e.add_effect_line(999, 1); // 未知词条静默跳过
        assert_eq!(e.youqing, 50.0);
    }

    /// 固有类型 21（103）：编成卡种数 >= 4 时训练效果 +10；卡种不足无加成
    ///
    /// 302704（[根]第一红宝）param `[103, 4, 8, 10]`。
    #[test]
    fn test_unique_effect_21() -> Result<()> {
        Data::init(data_root()?)?;
        // 编成 A：速×2 + 智 + 力 + 友 + 根(302704) = 5 种 >= 4
        let deck_a: [u32; 6] = [302424, 302464, 302484, 302564, 302574, 302704];
        let game_a = BasicGame::newgame(101901, &deck_a, InheritInfo::default(), &RACE_GRADES)?;
        let card = &game_a.deck[5];
        let base = CardTrainingEffect::from(card.card_value());
        let eff = card.calc_training_effect(&game_a, 3);
        assert_eq!(eff.xunlian, base.xunlian + 10, "编成 5 种卡时应触发训练 +10");
        println!("types>=4: xunlian {} -> {}", base.xunlian, eff.xunlian);

        // 编成 B：速×5 + 根(302704) = 2 种 < 4，无加成
        let deck_b: [u32; 6] = [200130, 200140, 200200, 200230, 200310, 302704];
        let game_b = BasicGame::newgame(101901, &deck_b, InheritInfo::default(), &RACE_GRADES)?;
        let card = &game_b.deck[5];
        let base = CardTrainingEffect::from(card.card_value());
        let eff = card.calc_training_effect(&game_b, 3);
        assert_eq!(eff.xunlian, base.xunlian, "编成仅 2 种卡时不应触发");
        Ok(())
    }

    /// 固有类型 5（105）：按编成支援卡类型加初始属性
    ///
    /// 302970（[力]爱丽数码）param `[105, 10, 2]`；对照卡 100050（[力]，
    /// 面板初始加成全零、无 5 型固有）。两局差值即纯 5 型加成。
    #[test]
    fn test_unique_effect_5() -> Result<()> {
        Data::init(data_root()?)?;
        let deck_b: [u32; 6] = [302424, 302464, 302484, 302564, 302574, 302970];
        let deck_ref: [u32; 6] = [302424, 302464, 302484, 302564, 302574, 100050];
        let game_b = BasicGame::newgame(101901, &deck_b, InheritInfo::default(), &RACE_GRADES)?;
        let game_ref = BasicGame::newgame(101901, &deck_ref, InheritInfo::default(), &RACE_GRADES)?;
        let diff: Vec<i32> = game_b
            .uma
            .five_status
            .iter()
            .zip(game_ref.uma.five_status)
            .map(|(a, b)| a - b)
            .collect();
        // 编成（含自身）：速×2 → 速+20；智+10；力×2 → 力+20；友 → 全维+2
        assert_eq!(diff, vec![22, 2, 22, 2, 12], "5 型初始属性加成（速2智1力2友1）");
        // 初始 PT 不受影响（两张对照卡面板 PT 均为 0）
        assert_eq!(game_b.uma.skill_pt, game_ref.uma.skill_pt);
        Ok(())
    }
}
