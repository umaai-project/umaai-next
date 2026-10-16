//! 基础（无剧本）游戏，用于测试特性与引擎冒烟
//!
//! 训练基础值表为内置默认（数值复刻温泉剧本表，保证与旧实现行为可对照）；
//! `race_grades` 由构造方注入（配置层归 P4′）。

use std::fmt::Display;
use std::ops::{Deref, DerefMut};
use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use enum_iterator::Sequence;
use rand::Rng;
use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use rand_distr::{Distribution, weighted::WeightedIndex};
use umaai_core::gamedata::EventData;
use umaai_core::{AttributeArray, Data, Ill};

use crate::game::base::{BaseAction, BaseGame, BasePerson};
use crate::game::{
    ActionEnum, FriendOutState, Game, InheritInfo, PersonType, SupportCard, Trainer, TrainingBasicTable,
    TrainingValueSource, TurnStage, Uma, UmaFlagsExt, extra_training_event, hint_attr_event, hint_skill_event,
};

/// 内置训练基础值表（数值复刻温泉剧本表：5 训练 × 5 等级 × [速耐力根智 PT 体力]）
static BASIC_TRAINING_TABLE: OnceLock<TrainingBasicTable> = OnceLock::new();

fn basic_training_table() -> &'static TrainingBasicTable {
    BASIC_TRAINING_TABLE.get_or_init(|| {
        vec![
            vec![
                [12, 0, 2, 0, 0, 7, -20],
                [14, 0, 3, 0, 0, 7, -25],
                [17, 0, 4, 0, 0, 7, -30],
                [20, 0, 5, 0, 0, 7, -35],
                [23, 0, 6, 0, 0, 7, -40],
            ],
            vec![
                [0, 11, 0, 3, 0, 7, -22],
                [0, 14, 0, 4, 0, 7, -27],
                [0, 17, 0, 5, 0, 7, -32],
                [0, 20, 0, 6, 0, 7, -37],
                [0, 23, 0, 8, 0, 7, -42],
            ],
            vec![
                [0, 3, 11, 0, 0, 7, -22],
                [0, 4, 14, 0, 0, 7, -27],
                [0, 5, 17, 0, 0, 7, -32],
                [0, 6, 20, 0, 0, 7, -37],
                [0, 8, 23, 0, 0, 7, -42],
            ],
            vec![
                [2, 0, 2, 12, 0, 7, -22],
                [2, 0, 2, 15, 0, 7, -27],
                [2, 0, 2, 18, 0, 7, -32],
                [3, 0, 3, 21, 0, 7, -37],
                [4, 0, 3, 24, 0, 7, -42],
            ],
            vec![
                [2, 0, 0, 0, 11, 7, 5],
                [2, 0, 0, 0, 12, 7, 5],
                [2, 0, 0, 0, 13, 7, 5],
                [3, 0, 0, 0, 14, 7, 5],
                [3, 0, 0, 0, 15, 7, 5],
            ],
        ]
    })
}

/// 基础动作包装：`Train` 需要在 `BasicGame` 上实现，其余转发 `BaseAction`
#[derive(Debug, Clone, PartialEq)]
pub struct BasicAction(pub BaseAction);

impl Deref for BasicAction {
    type Target = BaseAction;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for BasicAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ActionEnum for BasicAction {
    type Game = BasicGame;

    // Train 需要在当前类型里实现，其他操作可以使用 BaseAction 里的默认的实现
    fn apply(&self, game: &mut Self::Game, rng: &mut impl Rng) -> Result<()> {
        match &self.0 {
            BaseAction::Train(train) => self.do_train(game, *train as usize, rng),
            _ => self.0.apply(&mut game.base, rng),
        }
    }

    fn as_base_action(&self) -> Option<BaseAction> {
        self.0.as_base_action()
    }
}

impl BasicAction {
    /// 训练动作：失败判定 → 数值入账 → 羁绊 / 红点 / 加练 / 友人状态
    pub fn do_train(&self, game: &mut BasicGame, train: usize, rng: &mut impl Rng) -> Result<()> {
        // sanity check
        if train >= 5 {
            return Err(anyhow!("训练等级越界: {train}"));
        }
        let buffs = game.calc_training_buff(train)?;
        let failure_rate = game.calc_training_failure_rate(&buffs, train)? / 100.0;

        if rng.random_bool(failure_rate as f64) {
            // 再判断一次，如果还失败就是大失败
            if rng.random_bool(failure_rate as f64) {
                game.apply_event(&Data::get()?.system_event("training_fail_low")?.clone(), 0, rng)?;
                // 大失败：生病（不擅长训练）与 bool 同置
                game.uma_mut().flags.ill.insert(Ill::BAD_TRAINER);
                game.uma_mut().flags.bad_trainer = true;
            } else {
                game.apply_event(&Data::get()?.system_event("training_fail")?.clone(), 0, rng)?;
            }
        } else {
            let value = game.calc_training_value(&buffs, train)?;
            game.uma_mut().add_value(&value);
            // 增加训练次数
            game.base.train_level_count[train] += 1;
            // 增加羁绊：按人头类型给基值（支援卡 +7 / 理事长·记者 +5）；
            // 爱娇的全局 +2 由 add_friendship 统一施加
            let mut hint_persons = vec![];
            let mut friend_clicked = false;
            let mut yayoi_clicked = false;
            let mut reporter_clicked = false;
            for person_index in game.base.distribution[train].clone() {
                // 跳过空位（-1 表示空位）
                if person_index < 0 {
                    continue;
                }
                let p = person_index as usize;
                let base = match game.persons[p].person_type {
                    PersonType::Yayoi | PersonType::Reporter => 5,
                    _ => 7,
                };
                game.add_friendship(p, base);
                if game.persons[p].is_hint {
                    hint_persons.push(person_index);
                }
                match game.persons[p].person_type {
                    PersonType::ScenarioCard => friend_clicked = true,
                    PersonType::Yayoi => yayoi_clicked = true,
                    PersonType::Reporter => reporter_clicked = true,
                    _ => {}
                };
            }
            // 生成加练或者红点事件
            if let Some(p) = hint_persons.choose(rng) {
                let attr_prob = Data::get()?.event_prob("hint_attr")?;
                let hint_level = if (*p as usize) < game.deck.len() {
                    1 + game.deck[*p as usize].card_value().hint_level
                } else {
                    1
                };
                let mut hint_event = if rng.random_bool(attr_prob) {
                    // 红点提供属性
                    hint_attr_event(game.persons[*p as usize].train_type as usize, *p as usize)?
                } else {
                    // 红点提供技能
                    hint_skill_event(hint_level, *p as usize)
                };
                hint_event.name = format!("{} - {}", hint_event.name, game.deck[*p as usize].short_name());
                game.unresolved_events.push(hint_event);
            }
            let extra_train_prob = Data::get()?.event_prob("extra_train")?;
            if !game.is_xiahesu() && rng.random_bool(extra_train_prob) {
                let mut event = extra_training_event(train)?;
                // 动态设置理事长索引
                if let Some(yayoi_index) = game.persons.iter().position(|p| p.person_type == PersonType::Yayoi) {
                    event.person_index = Some(yayoi_index as i32);
                }
                game.unresolved_events.push(event);
            }
            // 更新友人状态
            if friend_clicked {
                match game.friend.out_state {
                    FriendOutState::UnClicked => {
                        game.friend.out_state = FriendOutState::BeforeUnlock;
                        let mut event = Data::get()?.events.friend_events["first"].clone();
                        event.person_index = Some(game.friend.person_index as i32); // 设置增加羁绊的目标为友人
                        game.unresolved_events.push(event);
                    }
                    _ => {
                        let mut event = Data::get()?.events.friend_events["click"].clone();
                        event.person_index = Some(game.friend.person_index as i32);
                        game.unresolved_events.push(event);
                    }
                }
            }
            if yayoi_clicked {
                game.unresolved_events
                    .push(Data::get()?.system_event("yayoi_click")?.clone());
            }
            if reporter_clicked {
                let mut event = Data::get()?.system_event("reporter_click")?.clone();
                event.choices[0][0].value.status_pt[train] = 2;
                game.unresolved_events.push(event);
            }
        }
        Ok(())
    }
}

/// 基础（无剧本）游戏
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BasicGame {
    /// 剧本通用的状态底座
    pub base: BaseGame,
    /// 人头列表
    pub persons: Vec<BasePerson>,
}

impl Deref for BasicGame {
    type Target = BaseGame;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for BasicGame {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

impl TrainingValueSource for BasicGame {
    /// 内置默认表（数值复刻温泉剧本表）
    fn training_basic_table(&self) -> &TrainingBasicTable {
        basic_training_table()
    }
}

impl BasicGame {
    /// 追加一个人头（分配人头序号）
    pub fn add_person(&mut self, mut person: BasePerson) {
        person.person_index = self.persons.len() as i32;
        self.persons.push(person);
    }

    /// 该回合是否生涯比赛回合
    pub fn is_race_turn(&self) -> bool {
        self.uma.is_race_turn(self.turn)
    }

    /// 建立一局基础游戏（`race_grades` 由配置层注入）
    /// 建立一局基础游戏（`deck_ids` 长度不固定，由调用方保证；`race_grades` 由配置层注入）
    pub fn newgame(uma_id: u32, deck_ids: &[u32], inherit: InheritInfo, race_grades: &[i32]) -> Result<Self> {
        let mut ret = BasicGame {
            base: BaseGame::new(
                uma_id,
                deck_ids,
                inherit,
                Data::get()?.consts.five_status_limit_base,
                race_grades,
            )?,
            persons: vec![],
        };
        ret.init_persons()?;
        Ok(ret)
    }

    /// 增加羁绊（爱娇的全局 +2 统一在此施加；同步支援卡面羁绊；上限 100）
    pub fn add_friendship(&mut self, person_index: usize, value: i32) {
        if person_index < self.persons.len() {
            // 爱娇：所有正向羁绊获取额外 +2（负向扣减不受影响）
            let value = if value > 0 && self.uma.flags.aijiao {
                value + 2
            } else {
                value
            };
            let new_value = (self.persons[person_index].friendship + value).min(100);
            self.persons[person_index].friendship = new_value;
            if person_index < self.deck.len() {
                self.deck[person_index].friendship = new_value;
            }
        }
    }
}

impl Game for BasicGame {
    type Person = BasePerson;
    type Action = BasicAction;

    fn init_persons(&mut self) -> Result<()> {
        let persons = self.deck.iter().map(BasePerson::try_from).collect::<Result<Vec<_>>>()?;
        for p in persons {
            self.add_person(p);
        }
        // 添加理事长
        self.add_person(BasePerson::yayoi());
        Ok(())
    }

    fn next(&mut self) -> bool {
        if let Some(stage) = self.stage.next() {
            // 回合内，下一个阶段
            self.stage = stage;
        } else if self.turn < self.max_turn() {
            // 下一个回合
            self.turn += 1;
            self.stage = TurnStage::Begin;
            // 检查自选比赛
            return self.check_free_race();
        } else {
            return false;
        }
        true
    }

    fn list_actions(&self) -> Result<Vec<Self::Action>> {
        use crate::game::base::BaseAction::{Clinic, FriendOuting, NormalOuting, Race, Sleep, Train};

        if self.is_race_turn() {
            Ok(vec![BasicAction(Race)])
        } else {
            let mut actions = vec![
                BasicAction(Train(0)),
                BasicAction(Train(1)),
                BasicAction(Train(2)),
                BasicAction(Train(3)),
                BasicAction(Train(4)),
            ];
            if self.is_xiahesu() {
                actions.push(BasicAction(Race));
                actions.push(BasicAction(Sleep));
            } else {
                // 普通训练
                actions.push(BasicAction(Sleep));
                actions.push(BasicAction(NormalOuting));
                // 历史行为：自 turn > 13 起可自选比赛（较 can_self_race 晚 1 回合）
                if self.turn > 13 && self.turn < 72 {
                    actions.push(BasicAction(Race));
                }
                if !self.uma.flags.ill.is_empty() {
                    actions.push(BasicAction(Clinic));
                }
                if self.friend.out_state == FriendOutState::AfterUnlock
                    && self.turn < 72
                    && !self.friend.out_used.iter().all(|used| *used)
                {
                    actions.push(BasicAction(FriendOuting));
                }
            }
            Ok(actions)
        }
    }

    fn generate_events(&self, rng: &mut impl Rng) -> Vec<EventData> {
        let data = match Data::get() {
            Ok(d) => d,
            Err(_) => return vec![],
        };
        let mut events = vec![];
        let no_event_turns = &data.consts.no_event_turns;
        // 剧本事件
        let story_events = self
            .list_turn_events(&data.events.story_events)
            .into_iter()
            .filter(|e| {
                matches!(
                    e.trigger,
                    umaai_core::gamedata::TriggerType::Random { .. } | umaai_core::gamedata::TriggerType::Fixed { .. }
                )
            })
            .collect::<Vec<_>>();
        if !story_events.is_empty() {
            // 如果有剧本事件，则返回剧本事件
            story_events
        } else if !no_event_turns.contains(&self.turn) {
            // 否则如果是允许发生随机事件的回合
            // 先判断友人出门事件
            if self.friend.out_state == FriendOutState::BeforeUnlock {
                let friendship = self.persons[self.friend.person_index].friendship;
                let out_prob = if friendship < 60 {
                    data.event_prob("friend_unlock_low")
                } else {
                    data.event_prob("friend_unlock_high")
                }
                .unwrap_or(0.0);
                if rng.random_bool(out_prob) {
                    events.push(data.events.friend_events["out"].clone());
                    return events;
                }
            }
            // 之后处理一般随机事件（分布与旧实现同一条 WeightedIndex 采样路径）
            let dist = WeightedIndex::new(data.consts.get_event_distribution()).expect("事件分布权重恒合法");
            match dist.sample(rng) {
                0 => {
                    // 支援卡事件. 再精细一点模拟 后一段事件发生次数不能多于前一段事件
                    let card_event_times: Vec<_> = [8001, 8002, 8003]
                        .iter()
                        .map(|x| *self.events.get(x).unwrap_or(&0))
                        .collect();
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
                        let mut event = data.events.card_events[*index].clone();
                        let mut p = rng.random_range(0..6);
                        while !self.person_is_available(p as usize) {
                            p = rng.random_range(0..6);
                        }
                        event.person_index = Some(p);
                        events.push(event);
                    }
                }
                1 => {
                    // 马娘事件
                    if let Some(event) = self.random_select_event(&data.events.uma_events, rng) {
                        events.push(event);
                    }
                }
                2 => {
                    // 掉心情事件
                    if self.turn >= 12
                        && let Ok(event) = data.system_event("drop_motivation")
                    {
                        events.push(event.clone());
                    }
                }
                _ => {
                    // 无事发生
                }
            }
            events
        } else {
            vec![]
        }
    }

    fn run_stage<T: Trainer<Self>>(&mut self, trainer: &T, rng: &mut StdRng) -> Result<()> {
        match self.stage {
            TurnStage::Begin => {
                let mut events = self.generate_events(rng);
                // 友人强制事件
                if self.friend.out_state == FriendOutState::AfterUnlock {
                    if self.turn == 24 {
                        events.push(Data::get()?.events.friend_events["newyear"].clone());
                    } else if self.turn == 77 {
                        self.unresolved_events
                            .push(Data::get()?.events.friend_events["end"].clone());
                    }
                }
                // 抽签事件
                if self.turn == 48 {
                    self.unresolved_events
                        .push(Data::get()?.system_event("ticket")?.clone());
                }
                // 育成结束事件
                if self.turn == 77 {
                    self.unresolved_events
                        .push(Data::get()?.system_event("ending")?.clone());
                }
                // 执行回合前事件
                for event in &events {
                    self.run_event(event, trainer, rng)?;
                }
            }
            TurnStage::Distribute => {
                if self.is_race_turn() {
                    self.reset_distribution();
                } else {
                    self.distribute_all(rng)?;
                    self.distribute_hint(rng)?;
                }
            }
            TurnStage::Train => {
                let actions = self.list_actions()?;
                let selection = trainer.select_action(self, &actions, rng)?;
                self.apply_action(&actions[selection], rng)?;
            }
            TurnStage::AfterTrain => {
                let after_events = std::mem::take(&mut self.unresolved_events);
                for event in &after_events {
                    self.run_event(event, trainer, rng)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// 使事件生效（无选项）。修改羁绊和特殊事件的部分需要在当前类型里完成
    fn apply_event(&mut self, event: &EventData, choice: usize, rng: &mut impl Rng) -> Result<()> {
        if let Some(result) = self.base.apply_event(event, choice, rng)
            && let Some(person_index) = &event.person_index
            && result.value.friendship != 0
        {
            self.add_friendship(*person_index as usize, result.value.friendship);
        }
        // 判断特殊事件
        match event.id {
            4012 | 4013 => {
                // 继承
                let inherit_value = umaai_core::gamedata::ActionValue {
                    status_pt: self.inherit.inherit(rng),
                    ..Default::default()
                };
                let inherit_limit = self.inherit.inherit_limit(rng);
                self.uma.add_value(&inherit_value);
                self.uma.five_status_limit.add_eq(&inherit_limit);
            }
            5007 => {
                // 大成功事件
                if rng.random_bool(Data::get()?.event_prob("qiezhe_normal")?) {
                    self.uma.flags.gain_qiezhe();
                }
            }
            809050004 => {
                // 友人出门事件
                self.friend.out_state = FriendOutState::AfterUnlock;
            }
            _ => {}
        }
        Ok(())
    }

    fn deyilv(&mut self, person_index: i32) -> f32 {
        // 人头下标 < 卡组长度 = 支援卡人头（依次对位 deck[i]）
        if person_index >= 0 && (person_index as usize) < self.deck.len() {
            // `calc_training_effect` 返回 owned cumulative——先取出 `deyilv`
            // 后直接 move 进 `self.deck[i].effect`（避免 clone）。
            let eff = self.deck[person_index as usize].calc_training_effect(self, 0);
            let deyilv = eff.deyilv;
            self.deck[person_index as usize].effect = eff;
            // is_locked 字段保留（NN feature 兼容），每次 deyilv 调用都标记
            self.deck[person_index as usize].is_locked = true;
            deyilv
        } else {
            0.0
        }
    }

    // getters
    fn persons(&self) -> &[Self::Person] {
        &self.persons
    }
    fn persons_mut(&mut self) -> &mut [Self::Person] {
        &mut self.persons
    }
    fn absent_rate_drop(&self) -> i32 {
        self.absent_rate_drop
    }
    fn turn(&self) -> i32 {
        self.turn
    }
    fn max_turn(&self) -> i32 {
        77
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
    fn distribution(&self) -> &Vec<Vec<i32>> {
        &self.base.distribution
    }
    fn distribution_mut(&mut self) -> &mut Vec<Vec<i32>> {
        &mut self.base.distribution
    }
    fn has_group_buff(&self) -> bool {
        self.friend.group_buff_turn > 0
    }
    fn train_level(&self, train: usize) -> usize {
        if self.is_xiahesu() {
            5
        } else {
            (self.train_level_count[train] as usize / 4 + 1).min(5).max(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;

    use super::*;
    use crate::testutil::RACE_GRADES;
    use umaai_core::testing::data_root;

    /// 测试用训练员（决策契约的最小实现）
    ///
    /// 动作优先级 Race > Clinic > Sleep > 其他：保证自选比赛达标、生病即治，
    /// 固定种子下可跑满 78 回合；事件选项恒选第 0 组。
    struct RaceFirstTrainer;

    impl Trainer<BasicGame> for RaceFirstTrainer {
        fn select_action(&self, _game: &BasicGame, actions: &[BasicAction], _rng: &mut StdRng) -> Result<usize> {
            let pri = |a: &BasicAction| match a.0 {
                BaseAction::Race => 0,
                BaseAction::Clinic => 1,
                BaseAction::Sleep => 2,
                _ => 3,
            };
            Ok(actions
                .iter()
                .enumerate()
                .min_by_key(|(_, a)| pri(a))
                .map(|(i, _)| i)
                .unwrap_or(0))
        }
        fn select_choice(
            &self,
            _game: &BasicGame,
            _choices: &[Vec<umaai_core::gamedata::EventChoice>],
            _rng: &mut StdRng,
        ) -> Result<usize> {
            Ok(0)
        }
    }

    /// 冒烟：固定种子跑满 78 回合（评分 / PT 输出供基线固化）
    #[test]
    fn test_newgame() -> Result<()> {
        Data::init(data_root()?)?;
        let mut game = BasicGame::newgame(
            101901,
            &[302424, 302464, 302484, 302564, 302574, 302644],
            InheritInfo {
                blue_count: [15, 3, 0, 0, 0],
                extra_count: [0, 30, 0, 0, 30, 30],
            },
            &RACE_GRADES,
        )?;
        assert_eq!(game.persons.len(), 7, "6 卡 + 理事长");

        let trainer = RaceFirstTrainer;
        let mut rng = StdRng::seed_from_u64(42);
        game.run_full_game(&trainer, &mut rng)?;
        assert_eq!(game.turn(), 77, "应跑满 78 回合（0-based 末回合 77）");

        let score = game.uma.calc_score()?;
        println!(
            "育成结束: score={} rank={} PT={} five={:?}",
            score,
            Data::get()?.consts.get_rank_name(score),
            game.uma.total_pt()?,
            game.uma.five_status
        );
        assert!(score > 0);
        Ok(())
    }

    /// 层1：友人出行用段推进（首段→次段）
    #[test]
    fn test_friend_outing_steps() -> Result<()> {
        Data::init(data_root()?)?;
        let mut game = BasicGame::newgame(
            101901,
            &[302424, 302464, 302484, 302564, 302574, 302644],
            InheritInfo::default(),
            &RACE_GRADES,
        )?;
        game.friend.out_state = FriendOutState::AfterUnlock;
        BasicAction(BaseAction::FriendOuting).apply(&mut game, &mut rand::rng())?;
        assert!(game.friend.out_used[0], "首段应被占用");
        assert!(!game.friend.out_used[1]);
        assert_eq!(game.unresolved_events.len(), 1, "出行事件入待触发队列");
        Ok(())
    }

    /// 层1：爱娇对所有正向羁绊获取统一 +2（训练与事件路径共用一个出口）
    #[test]
    fn test_aijiao_friendship_unified() -> Result<()> {
        Data::init(data_root()?)?;
        let mut game = BasicGame::newgame(
            101901,
            &[302424, 302464, 302484, 302564, 302574, 302644],
            InheritInfo::default(),
            &RACE_GRADES,
        )?;
        // 无爱娇：羁绊按传入值入账（训练基值 / 事件数值均原值）
        let (old0, old1, old2) = (
            game.persons[0].friendship,
            game.persons[1].friendship,
            game.persons[2].friendship,
        );
        game.add_friendship(0, 7);
        assert_eq!(game.persons[0].friendship, old0 + 7, "无爱娇训练羁绊 +7");
        game.add_friendship(1, 5);
        assert_eq!(game.persons[1].friendship, old1 + 5, "无爱娇事件羁绊原值");

        // 开爱娇：所有正向羁绊 +2
        game.uma.flags.aijiao = true;
        game.add_friendship(2, 7);
        assert_eq!(game.persons[2].friendship, old2 + 9, "爱娇下训练羁绊 7+2");
        let old3 = game.persons[3].friendship;
        game.add_friendship(3, 5);
        assert_eq!(game.persons[3].friendship, old3 + 7, "爱娇下事件羁绊也应 +2");
        // deck 面板羁绊同步
        assert_eq!(game.deck[2].friendship, game.persons[2].friendship);
        // 负向扣减不受爱娇影响
        let before = game.persons[0].friendship;
        game.add_friendship(0, -3);
        assert_eq!(game.persons[0].friendship, before - 3, "负向扣减不加爱娇");
        Ok(())
    }

    /// 层2：卡组长度不固定（5 张卡可构造并跑满整局）
    #[test]
    fn test_variable_deck_length() -> Result<()> {
        Data::init(data_root()?)?;
        let mut game = BasicGame::newgame(
            101901,
            &[302424, 302464, 302484, 302564, 302574],
            InheritInfo::default(),
            &RACE_GRADES,
        )?;
        assert_eq!(game.deck.len(), 5);
        assert_eq!(game.persons.len(), 6, "5 卡 + 理事长");
        game.run_full_game(&RaceFirstTrainer, &mut StdRng::seed_from_u64(42))?;
        assert_eq!(game.turn(), 77, "5 张卡也应跑满 78 回合");
        Ok(())
    }

    /// 层1：治病动作清空生病位集与不擅长训练
    #[test]
    fn test_clinic_clears_ill() -> Result<()> {
        Data::init(data_root()?)?;
        let mut game = BasicGame::newgame(
            101901,
            &[302424, 302464, 302484, 302564, 302574, 302644],
            InheritInfo::default(),
            &RACE_GRADES,
        )?;
        game.uma.flags.ill.insert(Ill::FAT);
        game.uma.flags.ill.insert(Ill::BAD_TRAINER);
        game.uma.flags.bad_trainer = true;
        let vital_before = game.uma.vital;
        BasicAction(BaseAction::Clinic).apply(&mut game, &mut rand::rng())?;
        assert!(game.uma.flags.ill.is_empty(), "治病应清空生病位集");
        assert!(!game.uma.flags.bad_trainer);
        assert_eq!(game.uma.vital, (vital_before + 20).min(game.uma.max_vital));
        Ok(())
    }
}
