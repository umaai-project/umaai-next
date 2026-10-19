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
use umaai_core::rng::{StrategyRng, StreamTag, derive_seed};
use umaai_core::{AttributeArray, Data, Ill};

use crate::game::base::{BaseAction, BaseGame, BasePerson, BasicStage, RuleStreams};
use crate::game::{
    ActionEnum, FriendOutState, Game, InheritInfo, StageId, SupportCard, Trainer, TrainingBasicTable,
    TrainingValueSource, Uma, UmaFlagsExt,
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
            // 点击后副作用（羁绊 / 红点 / 加练 / 友人·理事长·记者点击）统一走底座可复用件
            game.base.default_after_train(&mut game.persons, train, rng)?;
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
    /// 回合阶段（基础局私有的阶段机；真实剧本自持各自的阶段机）
    pub stage: BasicStage,
    /// 策略流：本局面的**动作结算**随机（训练成败 / 休息 / 外出）
    ///
    /// 这是**剧本轴**：引擎只提供决策流与局面 / 事件流，机制随机由各剧本自持。派生与重置
    /// 随 [`Game::set_rule_master`] 与回合边界统一走，外部不要直接改。
    pub strategy: Option<StrategyRng>,
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
            stage: BasicStage::Begin,
            strategy: None,
        };
        ret.init_persons()?;
        Ok(ret)
    }

    /// 单阶段推进：流已按轴取出，本函数只按阶段分派
    fn stage_body<T: Trainer<Self>>(
        &mut self,
        trainer: &T,
        decision: &mut StdRng,
        s: &mut RuleStreams,
        strategy: &mut StrategyRng,
    ) -> Result<()> {
        match self.stage {
            BasicStage::Begin => {
                let mut events = self.generate_events(&mut s.event);
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
                    self.run_event(event, trainer, decision, &mut s.event)?;
                }
            }
            BasicStage::Distribute => {
                if self.is_race_turn() {
                    self.reset_distribution();
                } else {
                    self.distribute_all(&mut s.turn_fixed)?;
                    self.distribute_hint(&mut s.turn_fixed)?;
                }
            }
            BasicStage::Train => {
                let actions = self.list_actions()?;
                let selection = trainer.select_action(self, &actions, decision)?;
                // 动作结算走策略流：与训练员抽签分开，改决策流消耗不会位移结算随机
                self.apply_action(&actions[selection], strategy)?;
            }
            BasicStage::AfterTrain => {
                let after_events = std::mem::take(&mut self.unresolved_events);
                for event in &after_events {
                    self.run_event(event, trainer, decision, &mut s.event)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// 按 `(master, turn)` 派生本局面自持的策略流（注入点与回合边界调用）
    fn reset_strategy_stream(&mut self) {
        let turn = self.base.turn as u64;
        self.strategy = self
            .base
            .master()
            .map(|master| StrategyRng::new(derive_seed(master, &[turn, StreamTag::Strategy.tag()])));
    }

    /// 友人的人头下标（按友人卡的 `card_id` 反查）
    ///
    /// 没带友人、或人头还没建（`init_persons` 之前）时为 `None`。
    /// 卡组下标与人头下标不保证相等，故一律按身份反查，不按下标对位。
    #[must_use]
    pub fn friend_person_index(&self) -> Option<usize> {
        self.friend.card_id.and_then(|id| self.person_index_of_card(id))
    }

    /// 增加羁绊（转调 [`BaseGame::add_friendship`]：爱娇 +2、上限 100、同步卡面）
    pub fn add_friendship(&mut self, person_index: usize, value: i32) {
        // persons 与 base 是两个独立字段，可同时借用
        self.base.add_friendship(&mut self.persons, person_index, value);
    }
}

impl Game for BasicGame {
    type Person = BasePerson;
    type Action = BasicAction;

    fn rule_master(&self) -> Option<u64> {
        self.base.master()
    }

    fn set_rule_master(&mut self, master: u64) {
        // 引擎两轴 + 本局面的策略轴一起重置（走 init_rule_streams 而不是同名方法：见其文档）
        self.base.init_rule_streams(master);
        self.reset_strategy_stream();
    }

    fn init_persons(&mut self) -> Result<()> {
        let persons = self.deck.iter().map(BasePerson::try_from).collect::<Result<Vec<_>>>()?;
        for p in persons {
            self.add_person(p);
        }
        // 添加理事长
        self.add_person(BasePerson::yayoi());
        Ok(())
    }

    fn stage(&self) -> StageId {
        self.stage.into()
    }

    fn next(&mut self) -> bool {
        if let Some(stage) = self.stage.next() {
            // 回合内，下一个阶段
            self.stage = stage;
        } else if self.turn < self.max_turn() {
            // 下一个回合
            self.turn += 1;
            self.stage = BasicStage::Begin;
            // 新回合：两条引擎流与自持的策略流一起按 `(master, 新回合)` 重新派生
            self.base.reset_rule_streams();
            self.reset_strategy_stream();
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
                // 历史行为：自 turn > 13 起可自选比赛（较 can_self_race 晚 1 回合）；
                // 等级表为 0 的回合没有赛事，不列比赛
                if self.turn > 13 && self.turn < 72 && self.race_grade().is_some() {
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
                let friendship = self
                    .friend_person_index()
                    .map_or(0, |index| self.persons[index].friendship);
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

    fn run_stage<T: Trainer<Self>>(&mut self, trainer: &T, decision: &mut StdRng) -> Result<()> {
        // 三条轴各就各位：局面流 / 事件流从底座取出，策略流本局面自持，决策流只给 Trainer。
        // 取出-放回成对执行（即使中途出错也先放回再传播），避免推进过的流状态被丢掉。
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

    /// 使事件生效（无选项）。修改羁绊和特殊事件的部分需要在当前类型里完成
    fn apply_event(&mut self, event: &EventData, choice: usize, rng: &mut impl Rng) -> Result<()> {
        if let Some(result) = self.base.apply_event(event, choice, rng)
            && result.value.friendship != 0
            && let Some(person_index) = self.base.event_person_target(event, self.persons())
        {
            self.add_friendship(person_index, result.value.friendship);
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
            (self.train_level_count[train] as usize / 4 + 1).clamp(1, 5)
        }
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;

    use crate::game::PersonType;

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
        game.set_rule_master(42);
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

    /// 层1：非卡人头按身份定位——位置随卡数走，不依赖固定下标
    #[test]
    fn test_person_index_lookup() -> Result<()> {
        Data::init(data_root()?)?;
        // 6 张（含第 5 槽的友人卡）：人头 = 6 卡 + 理事长
        let game = BasicGame::newgame(
            101901,
            &[302424, 302464, 302484, 302564, 302574, 302644],
            InheritInfo::default(),
            &RACE_GRADES,
        )?;
        assert_eq!(
            game.person_index_of_type(PersonType::Yayoi),
            Some(6),
            "理事长紧随卡片之后"
        );
        assert_eq!(game.person_index_of_type(PersonType::Reporter), None, "基础局没有记者");
        let friend = game.friend.card_id.expect("夹具含友人卡");
        assert_eq!(friend, 30257);
        assert_eq!(game.person_index_of_card(friend), Some(4), "友人卡在第 5 槽");
        assert_eq!(game.friend_person_index(), Some(4));

        // 8 张：理事长下标跟着卡数走（写死 6 就会指到第 7 张卡）
        let wide = BasicGame::newgame(
            101901,
            &[302424, 302464, 302484, 302564, 302574, 302644, 302704, 302974],
            InheritInfo::default(),
            &RACE_GRADES,
        )?;
        assert_eq!(wide.deck.len(), 8);
        assert_eq!(wide.persons.len(), 9, "8 卡 + 理事长");
        assert_eq!(wide.person_index_of_type(PersonType::Yayoi), Some(8));
        // 追加记者（剧本形态）后同样按类型查
        let mut with_reporter = wide.clone();
        with_reporter.add_person(BasePerson::reporter());
        assert_eq!(with_reporter.person_index_of_type(PersonType::Reporter), Some(9));
        Ok(())
    }

    /// 层1：点击事件的归属按类型动态查——8 卡局里理事长在第 9 位，数据里写死的 6 是错的
    #[test]
    fn test_click_event_person_target() -> Result<()> {
        Data::init(data_root()?)?;
        let mut game = BasicGame::newgame(
            101901,
            &[302424, 302464, 302484, 302564, 302574, 302644, 302704, 302974],
            InheritInfo::default(),
            &RACE_GRADES,
        )?;
        game.set_rule_master(42);
        game.add_person(BasePerson::reporter());
        let yayoi = game.person_index_of_type(PersonType::Yayoi).expect("应有理事长");
        let reporter = game.person_index_of_type(PersonType::Reporter).expect("应有记者");
        assert_eq!((yayoi, reporter), (8, 9), "非卡人头位于卡片之后、按加入顺序");

        // 只把目标人头放进 0 号训练位（其余清空）：点击事件由「该训练位上的人头类型」触发
        game.base.distribution = vec![Vec::new(); 5];
        game.base.distribution[0] = vec![yayoi as i32, reporter as i32];
        game.uma.vital = 100;
        game.uma.motivation = 5;

        let train = BasicAction(BaseAction::Train(0));
        let mut seen = (None, None);
        for seed in 0..128_u64 {
            let mut trial = game.clone();
            trial.unresolved_events.clear();
            train.do_train(&mut trial, 0, &mut StdRng::seed_from_u64(seed))?;
            for event in &trial.unresolved_events {
                if event.id == 901 {
                    seen.0 = event.person_index;
                }
                if event.id == 902 {
                    seen.1 = event.person_index;
                }
            }
            if seen.0.is_some() && seen.1.is_some() {
                break;
            }
        }
        assert_eq!(seen.0, Some(yayoi as i32), "点击理事长的归属应是实际人头下标");
        assert_eq!(seen.1, Some(reporter as i32), "点击记者的归属应是实际人头下标");
        Ok(())
    }

    /// 层1：友人事件没带 `person_index` 时按友人身份归属（`out` / `newyear` 的羁绊不再丢）
    #[test]
    fn test_friend_event_attribution() -> Result<()> {
        Data::init(data_root()?)?;
        let game = BasicGame::newgame(
            101901,
            &[302424, 302464, 302484, 302564, 302574, 302644],
            InheritInfo::default(),
            &RACE_GRADES,
        )?;
        let event = Data::get()?.events.friend_events["out"].clone();
        assert!(event.person_index.is_none(), "夹具事件本身不带人头下标");

        // 人为在卡片之前插入一个人头，模拟「剧本先加自己人头」的布局：
        // 卡组下标与人头下标就此错位，按身份反查仍要命中友人
        let mut shifted = game.clone();
        shifted.persons.insert(0, BasePerson::yayoi());
        let friend_index = shifted.friend_person_index().expect("按身份仍能反查到友人");
        assert_eq!(friend_index, 5, "友人卡在第 5 槽，错位后人头下标是 5（卡组下标仍是 4）");
        let before = shifted.persons[friend_index].friendship;
        shifted.set_rule_master(42);
        shifted.apply_event(&event, 0, &mut StdRng::seed_from_u64(7))?;
        let after = shifted.persons[friend_index].friendship;
        assert!(after > before, "友人事件应记到友人头上: {before} -> {after}");
        assert_eq!(
            shifted.deck[4].friendship, after,
            "卡面羁绊按 card_id 反查同步到第 5 槽"
        );
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
        game.set_rule_master(42);
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
