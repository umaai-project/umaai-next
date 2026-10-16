//! 基础游戏状态（`BaseGame`）：剧本通用的一局状态底座
//!
//! 不含人头信息（`Person` 类型因剧本而异），具体剧本对象需补上
//! `Vec<Person>` 与自己的字段才能实现 [`crate::game::Game`]。

pub mod action;
pub mod basic;
pub mod person;

use std::sync::Arc;

use anyhow::Result;
use hashbrown::{HashMap, HashSet};
use rand::Rng;
use rand::seq::IndexedRandom;
use umaai_core::gamedata::{ActionValue, EventChoice, EventData, TriggerType};
use umaai_core::{Array5, AttributeArray, Data, split_status};

pub use action::*;
pub use person::*;

use crate::game::{FriendOutState, FriendState, InheritInfo, SupportCard, TurnStage, Uma};

/// 一局游戏的基本状态，剧本通用，用于计算，不用于通信（例如通信只传递卡组id）
///
/// 需要频繁 clone，一部分不变量需要引用。序列化在 record 阶段（P4′）引入。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BaseGame {
    /// 回合数 [0, 77]
    pub turn: i32,
    /// 回合阶段
    pub stage: TurnStage,
    /// 马娘信息
    pub uma: Uma,
    /// 卡组信息
    pub deck: Vec<SupportCard>,
    /// 继承因子信息，在育成中不变但是要随时取
    pub inherit: InheritInfo,
    /// 友人数据
    pub friend: FriendState,
    /// 设施等级计数 (设施等级x4)
    pub train_level_count: Array5,
    /// 人头分布 [训练, persons_index]. -1为不在
    pub distribution: Vec<Vec<i32>>,
    /// 不在率下降，处理成加算
    pub absent_rate_drop: i32,
    /// 已经触发的事件id和次数
    pub events: HashMap<u32, u32>,
    /// 本回合内还没触发的事件(Hint, 点击友人等)
    pub unresolved_events: Vec<EventData>,
    /// 每种训练卡数量，用于训练倾向和固有判断
    pub card_type_count: [i32; 7],
    /// 友人事件 ID 集合（从 `events.friend_events` 派生）
    pub friend_event_ids: HashSet<u32>,
    /// 比赛等级表（回合 → 等级），归配置层，由构造方注入
    pub race_grades: Arc<Vec<i32>>,
}

impl BaseGame {
    /// 该回合是否允许**自选比赛**（通用规则，各剧本 list_actions 复用）
    ///
    /// 自选比赛仅限回合 13-71（0-based）；URA 回合（72-77）不可自选。
    /// 注意：`BasicGame::list_actions` 以 `turn > 13` 起始（较本规则晚 1 回合，
    /// 历史行为保留）。
    pub fn can_self_race(&self) -> bool {
        self.turn > 12 && self.turn < 72
    }

    /// 该回合是否允许**友人出行**（通用规则，各剧本 list_actions 复用）
    ///
    /// 条件：友人已解锁（AfterUnlock）且未进入 URA 回合（72-77）且出行次数未用完。
    pub fn can_friend_outing(&self) -> bool {
        self.friend.out_state == FriendOutState::AfterUnlock
            && self.turn < 72
            && !self.friend.out_used.iter().all(|used| *used)
    }

    /// 建立游戏对象
    ///
    /// `deck_ids` 长度不固定（剧本的卡组约定可能不同），由调用方保证合法性；
    /// `limit_base` 由调用的剧本提供（见 [`Uma::new`]）；`race_grades` 为比赛
    /// 等级表（配置层注入）。构造顺序是**先写剧本基值、再加继承**；任何剧本
    /// 都不得在本函数返回后整体赋值 `five_status_limit`，那会擦掉已累加的
    /// 开局继承增量。
    pub fn new(
        uma_id: u32,
        deck_ids: &[u32],
        inherit: InheritInfo,
        limit_base: Array5,
        race_grades: &[i32],
    ) -> Result<Self> {
        let mut uma = Uma::new(uma_id, limit_base, race_grades)?;
        let mut deck = vec![];
        let mut friend_id = None;
        let mut friend_index = 0;
        let mut card_type_count = [0; 7];
        // 支援卡
        for (index, id) in deck_ids.iter().enumerate() {
            let card = SupportCard::new(*id)?;
            // 初始属性
            let initial = card.initial_bonus();
            let race_bonus = card.effect.saihou;
            let (initial, pt) = split_status(initial)?;
            uma.five_status.add_eq(initial);
            uma.skill_pt += pt;
            uma.race_bonus += race_bonus;
            // 友人. 暂时不处理多个友人
            if card.card_type >= 5 {
                friend_id = Some(*id);
                friend_index = index;
            }
            // 记录训练类型
            if card.card_type < 7 {
                card_type_count[card.card_type as usize] += 1;
            }
            deck.push(card);
        }
        // 固有类型 5（105）：按编成支援卡类型加初始属性——
        // 编成中每张训练系卡（0-4）给对应属性 +param[1]，每张友人/团队卡（5+）全五维 +param[2]。
        // 逐张生效（编成含多张此类固有卡时叠加），按全编成（含该卡自身）计。
        for card in &deck {
            if card.data.unique_effect_type == 5 {
                let param = &card.data.unique_effect_param;
                for other in &deck {
                    if (0..5).contains(&other.card_type) {
                        uma.five_status[other.card_type as usize] += param[1];
                    } else {
                        for i in 0..5 {
                            uma.five_status[i] += param[2];
                        }
                    }
                }
            }
        }
        // 继承
        let newgame_inherit = inherit.inherit_newgame();
        let newgame_limit_inherit = inherit.inherit_limit_newgame();
        uma.five_status.add_eq(&newgame_inherit);
        uma.five_status_limit.add_eq(&newgame_limit_inherit);
        Ok(Self {
            turn: 0,
            stage: TurnStage::Begin,
            uma,
            deck,
            inherit,
            friend: FriendState::new(friend_id, friend_index)?,
            train_level_count: [0; 5],
            distribution: vec![],
            events: HashMap::new(),
            absent_rate_drop: 0,
            unresolved_events: vec![],
            card_type_count,
            // 从 events.friend_events 派生友人事件 ID
            friend_event_ids: Data::get()?.events.friend_events.values().map(|e| e.id).collect(),
            race_grades: Arc::new(race_grades.to_vec()),
        })
    }

    /// 基础设施等级（计数 / 4 + 1，夹 1-5）
    pub fn base_train_level(&self, train: usize) -> usize {
        (self.train_level_count[train] / 4 + 1).clamp(0, 5) as usize
    }

    /// 随机选择一个能发生的事件（按 `max_time` 次数限制过滤）
    pub fn random_select_event(&self, events: &[EventData], rng: &mut impl Rng) -> Option<EventData> {
        let available_events: Vec<_> = events
            .iter()
            .filter(|e| {
                if let TriggerType::Random { max_time } = &e.trigger {
                    *max_time == 0 || *self.events.get(&e.id).unwrap_or(&0) < *max_time
                } else {
                    true
                }
            })
            .collect();
        available_events.choose(rng).map(|e| (*e).clone())
    }

    /// 已经选择了选项组 choice，按概率随机决定组内结果
    pub fn random_select_choice_result(&self, choices: &[EventChoice], rng: &mut impl Rng) -> Option<EventChoice> {
        if choices.is_empty() {
            None
        } else if choices.len() == 1 {
            Some(choices[0].clone())
        } else {
            choices.choose_weighted(rng, |c| c.prob).ok().cloned()
        }
    }

    /// 使事件生效，并随机决定结果，返回实际生效的效果
    pub fn apply_event(&mut self, event: &EventData, choice: usize, rng: &mut impl Rng) -> Option<EventChoice> {
        self.events.entry(event.id).and_modify(|x| *x += 1).or_insert(1);
        if !event.choices.is_empty() {
            if let Some(mut choice_result) = self.random_select_choice_result(&event.choices[choice], rng) {
                // 友人事件：应用 friend.event_bonus / vital_bonus 乘算
                if self.friend_event_ids.contains(&event.id) {
                    Self::apply_friend_bonus(&mut choice_result.value, &self.friend);
                }
                self.uma.add_value(&choice_result.value);
                Some(choice_result)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// 对 ActionValue 应用友人卡词条 bonus 乘算
    ///
    /// - `event_bonus`（「事件效果提高」词条）：仅对五维 + pt 乘算（floor 除法），
    ///   不影响 vital / motivation / hint。
    /// - `vital_bonus`（「恢复量提高」词条）：仅对**正向**体力恢复乘算，
    ///   负向体力消耗不受影响。
    fn apply_friend_bonus(value: &mut ActionValue, friend: &FriendState) {
        if friend.event_bonus != 0 {
            let multiplier = 100 + friend.event_bonus;
            for i in 0..6 {
                if value.status_pt[i] != 0 {
                    value.status_pt[i] = value.status_pt[i] * multiplier / 100;
                }
            }
        }
        if friend.vital_bonus != 0 && value.vital > 0 {
            let multiplier = 100 + friend.vital_bonus;
            value.vital = value.vital * multiplier / 100;
        }
    }

    /// 结算某个事件选项（数值 + 状态）
    pub fn apply_event_choice(&mut self, choice: &EventChoice) {
        self.uma.add_value(&choice.value);
        self.uma.update_flags(choice);
    }

    /// 是否小合宿回合（36-39 / 60-63）
    pub fn is_xiahesu(&self) -> bool {
        (self.turn >= 36 && self.turn < 40) || (self.turn >= 60 && self.turn < 64)
    }

    /// 生成支援卡事件（后一段事件发生次数不能多于前一段）；无可用事件返回 `None`
    pub fn generate_card_event(&self, person_index: i32, rng: &mut impl Rng) -> Result<Option<EventData>> {
        let card_event_times = [8001, 8002, 8003].map(|id| *self.events.get(&id).unwrap_or(&0));
        let mut available_events = vec![];
        if card_event_times[0] < 5 {
            available_events.push(0);
        }
        if card_event_times[1] < card_event_times[0] {
            available_events.push(1);
        }
        if card_event_times[2] < card_event_times[1] {
            available_events.push(2);
        }
        if let Some(index) = available_events.choose(rng) {
            let mut event = Data::get()?.events.card_events[*index].clone();
            event.person_index = Some(person_index);
            Ok(Some(event))
        } else {
            Ok(None)
        }
    }

    /// 检测自选比赛是否达标（只在各区间结束回合 +1 时检测）
    pub fn check_free_race(&self) -> bool {
        for free_race in &self.uma.free_races {
            if self.turn as u32 == free_race.end_turn + 1 && self.uma.count_free_race(free_race) < free_race.count {
                return false;
            }
        }
        true
    }

    /// 比较 self 是否为 last 的同一或者下一回合（在线记录对齐用）
    pub fn is_next_of(&self, last: &BaseGame) -> bool {
        // 马娘ID相同且回合数相同或差1，则再检查卡组（长度与内容均需一致）
        if self.uma.uma_id == last.uma.uma_id && (self.turn == last.turn || self.turn == last.turn + 1) {
            return self.deck.len() == last.deck.len()
                && self.deck.iter().zip(&last.deck).all(|(a, b)| a.card_id == b.card_id);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::RACE_GRADES;
    use umaai_core::testing::data_root;

    fn newgame() -> Result<BaseGame> {
        Data::init(data_root()?)?;
        BaseGame::new(
            101901,
            &[302424, 302464, 302484, 302564, 302574, 302644],
            InheritInfo {
                blue_count: [15, 3, 0, 0, 0],
                extra_count: [0, 30, 0, 0, 30, 30],
            },
            Data::get()?.consts.five_status_limit_base,
            &RACE_GRADES,
        )
    }

    /// 层1：自选比赛回合边界
    #[test]
    fn test_can_self_race_bounds() {
        let mut game = BaseGame { turn: 12, ..Default::default() };
        assert!(!game.can_self_race(), "回合 12 不可自选");
        game.turn = 13;
        assert!(game.can_self_race(), "回合 13 起可自选");
        game.turn = 71;
        assert!(game.can_self_race(), "回合 71 仍可自选");
        game.turn = 72;
        assert!(!game.can_self_race(), "URA 回合不可自选");
    }

    /// 层1：友人出行条件边界
    #[test]
    fn test_can_friend_outing_bounds() {
        let mut game = BaseGame::default();
        game.friend.out_state = FriendOutState::AfterUnlock;
        game.turn = 71;
        game.friend.out_used = vec![false; 5];
        assert!(game.can_friend_outing());
        game.turn = 72;
        assert!(!game.can_friend_outing(), "URA 回合不可出行");
        game.turn = 30;
        game.friend.out_used = vec![true; 5];
        assert!(!game.can_friend_outing(), "出行次数用完");
        game.friend.out_used = vec![true, false, true, false, true];
        assert!(game.can_friend_outing(), "还有余量");
        game.friend.out_state = FriendOutState::BeforeUnlock;
        assert!(!game.can_friend_outing(), "未解锁");
    }

    /// 层1：apply_friend_bonus 对 status_pt 乘算（floor 除法）
    #[test]
    fn test_apply_friend_bonus_status_pt() {
        let mut value = ActionValue {
            status_pt: [100, 0, 50, -50, 33, 10],
            ..Default::default()
        };
        let friend = FriendState {
            event_bonus: 30,
            ..Default::default()
        };
        BaseGame::apply_friend_bonus(&mut value, &friend);
        // 100*130/100=130；0 跳过；50→65；-50→-65；33→42（floor）；10→13
        assert_eq!(value.status_pt, [130, 0, 65, -65, 42, 13]);
    }

    /// 层1：apply_friend_bonus 只对正向 vital 乘算
    #[test]
    fn test_apply_friend_bonus_vital() {
        let mut value = ActionValue {
            vital: 30,
            ..Default::default()
        };
        let friend = FriendState {
            vital_bonus: 40,
            ..Default::default()
        };
        BaseGame::apply_friend_bonus(&mut value, &friend);
        assert_eq!(value.vital, 42, "30*140/100=42");

        let mut value = ActionValue {
            vital: -30,
            ..Default::default()
        };
        BaseGame::apply_friend_bonus(&mut value, &friend);
        assert_eq!(value.vital, -30, "负向体力消耗不受加成");
    }

    /// 层1：无加成时值不变
    #[test]
    fn test_apply_friend_bonus_no_bonus() {
        let mut value = ActionValue {
            status_pt: [10, 20, 30, 40, 50, 60],
            vital: 30,
            ..Default::default()
        };
        BaseGame::apply_friend_bonus(&mut value, &FriendState::default());
        assert_eq!(value.status_pt, [10, 20, 30, 40, 50, 60]);
        assert_eq!(value.vital, 30);
    }

    /// 层2：构造一局（初始属性 / 继承 / 友人事件 ID 派生 / 等级表注入）
    #[test]
    fn test_newgame() -> Result<()> {
        let game = newgame()?;
        assert_eq!(game.turn, 0);
        assert_eq!(game.stage, TurnStage::Begin);
        assert_eq!(game.deck.len(), 6);
        assert_eq!(game.race_grades.len(), 72);
        // 初始五维 = 面板初始 + 卡组初始加成 + 继承（蓝因子 ×7），独立重算对照
        let mut expect_five = game.uma.get_data()?.five_status_initial;
        for card in &game.deck {
            let (initial, _) = split_status(card.initial_bonus())?;
            expect_five.add_eq(initial);
        }
        expect_five.add_eq(&[105, 21, 0, 0, 0]);
        assert_eq!(game.uma.five_status, expect_five);
        // 友人事件 ID 从 events.friend_events 派生（非空）
        assert!(!game.friend_event_ids.is_empty());
        println!(
            "newgame: five={:?} pt={} limit={:?} friend_ids={}",
            game.uma.five_status,
            game.uma.skill_pt,
            game.uma.five_status_limit,
            game.friend_event_ids.len()
        );
        Ok(())
    }

    /// 层2：apply_event 集成（计数 + 友人事件加成 + 数值入账）
    #[test]
    fn test_apply_event_friend_bonus_integration() -> Result<()> {
        let mut game = newgame()?;
        // 取一个友人事件（id 在 friend_event_ids 中且带选项）
        let friend_event = Data::get()?
            .events
            .friend_events
            .values()
            .find(|e| !e.choices.is_empty())
            .cloned()
            .expect("应存在带选项的友人事件");
        let (before_pt, before_five) = (game.uma.skill_pt, game.uma.five_status);
        let before_count = game.events.get(&friend_event.id).copied().unwrap_or(0);
        let result = game.apply_event(&friend_event, 0, &mut rand::rng());
        assert_eq!(
            game.events.get(&friend_event.id),
            Some(&(before_count + 1)),
            "事件计数 +1"
        );
        if let Some(result) = result {
            println!("友人事件 #{} 生效: {:?}", friend_event.id, result.value);
        }
        // 该事件带 status_pt（五维 / pt），入账后数值应有变化
        assert_ne!(
            (game.uma.skill_pt, game.uma.five_status),
            (before_pt, before_five),
            "带数值项的友人事件应入账"
        );
        Ok(())
    }
}
