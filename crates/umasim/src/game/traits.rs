//! 游戏契约 trait：引擎与剧本之间的接缝
//!
//! `Game` 为核心契约（含训练值公式与人头分配的默认实现）；
//! `TrainingValueSource` 是训练基础值表来源钩子，作为 `Game` 的 supertrait
//! （每个游戏必然提供表来源，默认公式才能取表）。`Trainer` 为决策侧契约。
//! 呈现 / 协议输出（`view` / 决策上下文等）不在本层，待上层阶段加回。

use std::fmt::{Debug, Display};

use anyhow::{Result, anyhow};
use rand::Rng;
use rand::rngs::StdRng;
use rand_distr::{Distribution, Uniform};
use umaai_core::Data;
use umaai_core::gamedata::{ActionValue, EventChoice, EventData, TriggerType};
use umaai_core::rng::EventRng;

use super::PersonType;
use crate::game::{CardTrainingEffect, StageId, SupportCard, Uma};

/// 训练基础值表：`[训练 0-4][等级 0-4] -> [速耐力根智 PT 体力消耗]`
pub type TrainingBasicTable = Vec<Vec<[i32; 7]>>;

/// 训练基础值表来源（剧本钩子）
///
/// 「无剧本」基础局由 [`crate::game::base`] 的 `BasicGame` 提供内置默认表；
/// 具体剧本在自己的 `Game` 实现上返回专属表。数值表归剧本数据，
/// 引擎侧不读任何具体剧本的全局数据。
pub trait TrainingValueSource {
    /// 取本游戏的训练基础值表
    fn training_basic_table(&self) -> &TrainingBasicTable;
}

/// 训练人头特性，用于随机分配
pub trait Person: Debug + Clone + PartialEq + Default {
    /// person type getter
    fn person_type(&self) -> PersonType;

    /// person index getter
    fn person_index(&self) -> i32;

    /// train type getter
    fn train_type(&self) -> i32;

    /// friendship getter
    fn friendship(&self) -> i32;

    /// hint getter
    fn hint(&self) -> bool;

    /// hint setter
    fn set_hint(&mut self, hint: bool);

    /// friendship setter
    ///
    /// 与 [`Person::set_hint`] 对称：羁绊的写入必须经统一出口
    /// （`BaseGame::add_friendship`），由出口负责爱娇加成、上限与卡面同步，调用方不要直接改。
    fn set_friendship(&mut self, friendship: i32);

    /// 支援卡 ID getter。非支援卡人头（理事长 / 记者 / NPC）返回 `None`。
    ///
    /// 用于把人头反查回卡组槽位，见 [`Game::deck_index_of`]。
    fn card_id(&self) -> Option<u32>;

    /// provided: 是否为友人，团队，记者或者理事长
    fn is_friend(&self) -> bool {
        self.train_type() > 4 || matches!(self.person_type(), PersonType::Reporter | PersonType::Yayoi)
    }

    /// 是否为剧本友人
    fn is_scenario_card(&self) -> bool {
        self.person_type() == PersonType::ScenarioCard
    }
}

/// 会改变 Game 状态的主动选项
pub trait ActionEnum: Debug + Display + Clone + PartialEq {
    /// 操作的对象类型，不一定要实现 Game trait
    type Game;

    /// visitor，调用具体动作
    fn apply(&self, game: &mut Self::Game, rng: &mut impl Rng) -> Result<()>;

    /// 尝试转变为 BaseAction 以获取基础行动类型
    fn as_base_action(&self) -> Option<crate::game::BaseAction> {
        None
    }
}

/// 游戏状态类型需要实现的 trait，不包括初始化
///
/// 对事件的处理由 Game 自身完成；训练值公式与人头分配提供默认实现，
/// 剧本按需 override（钩子方法均有默认值，见方案 v2 的契约纪律）。
pub trait Game: Clone + TrainingValueSource {
    /// 人头类型
    type Person: Person;
    /// 动作类型
    type Action: ActionEnum<Game = Self>;

    /// 当前回合
    fn turn(&self) -> i32;
    /// 最大回合数 getter
    fn max_turn(&self) -> i32;
    /// 规则主种子；未注入时返回 `None`
    ///
    /// 引擎的**局面流**与**事件流**都按 `(master, turn)` 每回合派生（见
    /// 基座入口 `init_rule_streams`）；剧本自己的机制随机也从这里取基点派生，或直接
    /// 复用规则层：**决策流只喂 `Trainer`**，局面/事件随机绝不与决策抽签共用一条流。
    fn rule_master(&self) -> Option<u64>;
    /// 注入规则主种子，并按当前回合派生出规则流
    ///
    /// 注入点：整局入口（`Runner` / `Session`）与搜索分支（`FlatSearchGame::fork_for_rollout`）。
    /// 未注入就推进会**报错**，不静默回退单流。
    fn set_rule_master(&mut self, master: u64);
    /// 当前阶段位置（剧本自定的数字标识）
    ///
    /// 阶段**只表示位置**，不承载中间状态（吃哪碗面、选了哪个地区在局面字段里）；
    /// 剧本内部用强类型枚举，对外投影成本类型。引擎只把它透传给前端，不做分支。
    fn stage(&self) -> StageId;
    /// 下一阶段。如果已经结束，返回false
    fn next(&mut self) -> bool;
    /// 模拟当前Stage
    fn run_stage<T: Trainer<Self>>(&mut self, trainer: &T, rng: &mut StdRng) -> Result<()>;
    /// provided: 模拟到游戏结束（含育成结束钩子）
    fn run_full_game<T: Trainer<Self>>(&mut self, trainer: &T, rng: &mut StdRng) -> Result<()> {
        self.run_stage(trainer, rng)?;
        while self.next() {
            self.run_stage(trainer, rng)?;
        }
        // 触发育成结束奖励
        self.on_simulation_end(trainer, rng)?;
        Ok(())
    }
    /// 育成结束时的处理（如最终奖励）；默认空，由具体剧本覆盖
    fn on_simulation_end<T: Trainer<Self>>(&mut self, _trainer: &T, _rng: &mut StdRng) -> Result<()> {
        Ok(())
    }
    /// 获取当前可能的可控行动
    fn list_actions(&self) -> Result<Vec<Self::Action>>;
    /// 生成当前回合的事件
    fn generate_events(&self, rng: &mut impl Rng) -> Vec<EventData>;
    /// 应用事件效果，一些特殊事件需要用到rng和Result
    fn apply_event(&mut self, event: &EventData, choice: usize, rng: &mut impl Rng) -> Result<()>;
    /// 执行事件，如果有选项，交给 Trainer 去决定
    ///
    /// 两条流分工：`player_select = true` 且多选项时用**决策流** `decision` 调 Trainer 决策；
    /// 事件结果一律用**事件流** `event_rng` 结算。合并成一条会让"选得多/少"位移事件结果，
    /// 也会让同一回合各分支的随机性互相污染。
    fn run_event<T: Trainer<Self>>(
        &mut self,
        event: &EventData,
        trainer: &T,
        decision: &mut StdRng,
        event_rng: &mut EventRng,
    ) -> Result<()> {
        if event.player_select && event.choices.len() > 1 {
            let selection = trainer.select_event_choice(self, event, &event.choices, decision)?;
            if selection >= event.choices.len() {
                return Err(anyhow!(
                    "事件选项索引超出范围: selection={}, choices_len={}, event#{} {}",
                    selection,
                    event.choices.len(),
                    event.id,
                    event.name
                ));
            }
            self.apply_event(event, selection, event_rng)
        } else {
            self.apply_event(event, 0, event_rng)
        }
    }
    /// provided: 列出本回合触发的事件（随机 / 固定回合触发）
    fn list_turn_events(&self, events: &[EventData]) -> Vec<EventData> {
        events
            .iter()
            .filter_map(|e| match &e.trigger {
                TriggerType::Random { .. } => Some(e.clone()),
                TriggerType::Code => None,
                TriggerType::Fixed { turns } => {
                    if turns.contains(&self.turn()) {
                        Some(e.clone())
                    } else {
                        None
                    }
                }
            })
            .collect()
    }
    /// provided: 执行指定动作
    ///
    /// 传进来的 `rng` 应当是**策略流**（动作结算随机），不是决策流：本方法只负责把动作
    /// 落到局面上，取哪条流由调用方（各剧本的 `run_stage`）决定。
    fn apply_action(&mut self, action: &Self::Action, rng: &mut impl Rng) -> Result<()> {
        action.apply(self, rng)
    }
    /// persons getter
    fn persons(&self) -> &[Self::Person];
    /// persons mut
    fn persons_mut(&mut self) -> &mut [Self::Person];
    /// 初始化人头
    fn init_persons(&mut self) -> Result<()>;
    /// 已经初始化的人头是否能出现在训练（如记者）
    fn person_is_available(&self, person_index: usize) -> bool {
        match self.persons()[person_index].person_type() {
            PersonType::ScenarioCard => self.turn() >= 2,
            PersonType::Reporter => self.turn() >= 13,
            _ => true,
        }
    }
    /// distribution getter
    fn distribution(&self) -> &Vec<Vec<i32>>;
    /// distribution mut
    fn distribution_mut(&mut self) -> &mut Vec<Vec<i32>>;
    /// absent_rate_drop getter
    fn absent_rate_drop(&self) -> i32;
    /// 计算得意率，同时修改卡片计算状态所以要 mut
    ///
    /// 不传 `Result`：函数体内无 fail 路径（人头下标落在**卡组范围**内时走计算返回 f32，
    /// 其余（理事长 / 追加人头 / 越界）返回 0.0）；与带防御性 Err 的 calc 系列形成分工。
    fn deyilv(&mut self, person_index: i32) -> f32;
    /// 团队卡是否可以闪彩，不考虑多个团卡的情况
    fn has_group_buff(&self) -> bool;
    /// 重置分布和叹号（保留各训练位容量）
    fn reset_distribution(&mut self) {
        let distribution = self.distribution_mut();
        distribution.resize_with(5, Vec::new);
        for train in distribution {
            train.clear();
        }
        for p in self.persons_mut() {
            p.set_hint(false);
        }
    }
    /// 人头「不在」权重（先判定不出现用，不含得意率）
    ///
    /// 支援卡基础 50、友人 / 团队卡基础 100（均可被 `absent_rate_drop` 降低）；
    /// 记者 / 理事长固定 200；NPC 必定出现（返回 0）。
    fn absent_weight(&self, person_index: usize) -> i32 {
        match self.persons()[person_index].person_type() {
            PersonType::Card => 50 - self.absent_rate_drop(),
            PersonType::Yayoi | PersonType::Reporter => 200,
            PersonType::ScenarioCard | PersonType::OtherFriend | PersonType::TeamCard => 100 - self.absent_rate_drop(),
            PersonType::Npc => 0,
        }
    }
    /// 记录本回合判定为「不在」的人头（供剧本后续机制计算）；默认空实现
    fn record_absent_person(&mut self, _person_index: i32) {}
    /// 追加分配一个在 persons 里已经存在的人头，-1 为不在
    ///
    /// 第一步先判「不在」（分母为 5 个训练位基础权重之和，不含得意率）；
    /// 第二步按训练位权重（含得意率）随机分配位置，约束：不多于 5 人、
    /// 同训练位无重复人头、每个训练位至多一个友人。
    fn distribute_person(&mut self, person_index: i32, allow_absent: bool, rng: &mut impl Rng) -> Result<i32> {
        let train_type = self.persons()[person_index as usize].train_type() as usize;
        let is_friend = self.persons()[person_index as usize].is_friend();
        let absent_rate = if allow_absent {
            self.absent_weight(person_index as usize)
        } else {
            0
        };
        if absent_rate > 0 && rng.random_bool(absent_rate as f64 / (500 + absent_rate) as f64) {
            self.record_absent_person(person_index);
            return Ok(-1);
        }
        // 已判定出现，按训练位权重（含得意率）随机分配
        let mut weights = [100, 100, 100, 100, 100];
        if train_type <= 4 {
            weights[train_type] += self.deyilv(person_index) as i32;
        }
        let d = self.distribution();
        let mut ok = false;
        let mut retries = 0;
        let mut train = 0;
        while !ok && retries < 10 {
            train = sample_bucket(&weights, rng)?;
            retries += 1;
            // 不能多于5人或出现同样人头
            if d[train].len() >= 5 || d[train].contains(&person_index) {
                continue;
            }
            // 每个训练只能出现一个友人
            if is_friend && d[train].iter().any(|index| self.persons()[*index as usize].is_friend()) {
                continue;
            }
            ok = true;
        }
        if !ok {
            Ok(-1)
        } else {
            self.distribution_mut()[train].push(person_index);
            Ok(train as i32)
        }
    }
    /// 重新分配所有人头（按人头类型顺序）
    fn distribute_all(&mut self, rng: &mut impl Rng) -> Result<()> {
        let sequence = vec![
            PersonType::Yayoi,
            PersonType::Reporter,
            PersonType::ScenarioCard,
            PersonType::TeamCard,
            PersonType::Card,
            PersonType::Npc,
        ];
        self.reset_distribution();
        for ty in sequence {
            for i in 0..self.persons().len() {
                if self.persons()[i].person_type() == ty && self.person_is_available(i) {
                    // NPC 必定出现：跳过不在判定（absent_weight 对 Npc 返回 0 双保险）
                    let allow_absent = self.persons()[i].person_type() != PersonType::Npc;
                    self.distribute_person(i as i32, allow_absent, rng)?;
                }
            }
        }
        Ok(())
    }
    /// 分配 Hint（基础公式：基础发生率 × 卡面 `hint_prob_increase` 加算）
    ///
    /// 基础公式**不包含剧本修正**：剧本要改发生率算法（如温泉的源泉加成）时，整体覆写本方法、
    /// 在自身实现里维护实际公式，不要让默认实现去猜系数。
    ///
    /// 注意同一个卡的不同分身会同时触发 Hint；红点**发生次数**（`hint_count_bonus`）与
    /// **红点等级**（`hint_level`）是另外两套机制，不在这里处理。
    fn distribute_hint(&mut self, rng: &mut impl Rng) -> Result<()> {
        let base_hint_rate = Data::get()?.consts.base_hint_rate / 100.0;
        // 人头下标 ≠ 卡组下标：预抽 (card_id, hint 概率加成)，循环内按 card_id 反查
        let hint_probs: Vec<(u32, i32)> = self
            .deck()
            .iter()
            .map(|card| (card.card_id, card.card_value().hint_prob_increase))
            .collect();
        for person in self.persons_mut() {
            if person.person_type() == PersonType::Card {
                let bonus = person
                    .card_id()
                    .and_then(|cid| hint_probs.iter().find(|(id, _)| *id == cid))
                    .map_or(0, |(_, bonus)| *bonus);
                let hint_prob = base_hint_rate * ((100 + bonus) as f64 / 100.0);
                person.set_hint(rng.random_bool(hint_prob));
            }
        }
        Ok(())
    }
    /// provided: 指定人头出现在训练中的位置
    fn at_trains(&self, person_index: i32) -> Vec<bool> {
        self.distribution()
            .iter()
            .map(|train| train.contains(&person_index))
            .collect()
    }
    /// provided: 指定人头如果在指定位置是否会闪彩（train 0-4 速耐力根智）
    ///
    /// 非默认实现需要依赖于一部分剧本 Buff，所以要在 Game 里判断。
    fn is_shining_at(&self, person_index: usize, train: usize) -> bool {
        let person = &self.persons()[person_index];
        match person.person_type() {
            PersonType::Card => person.train_type() == train as i32 && person.friendship() >= 80,
            PersonType::TeamCard => self.has_group_buff(),
            // 默认实现中其他卡不能闪彩
            _ => false,
        }
    }
    /// provided: 指定训练的彩圈个数
    fn shining_count(&self, train: usize) -> usize {
        // 防御：distribution 未初始化时返回 0
        self.distribution()
            .get(train)
            .map(|d| {
                d.iter()
                    .filter(|index| **index >= 0) // 过滤掉分身（负数）和空位（-1）
                    .filter(|index| self.is_shining_at(**index as usize, train))
                    .count()
            })
            .unwrap_or(0)
    }
    /// 设施等级 getter
    fn train_level(&self, train: usize) -> usize;
    /// uma getter
    fn uma(&self) -> &Uma;
    /// uma mut getter
    fn uma_mut(&mut self) -> &mut Uma;
    /// deck getter
    fn deck(&self) -> &Vec<SupportCard>;
    /// provided: 按人头类型找第一个匹配的人头下标（理事长 / 记者这类非卡人头）
    ///
    /// 人头位置由各自的 `init_persons` 决定（基础局是「卡片人头在前、理事长随后」，
    /// 剧本还可能追加自己的友人），**不要按固定下标取**：卡数一变下标就漂。
    /// 找不到返回 `None`，调用方自行决定降级（不归属 / 跳过该事件）。
    fn person_index_of_type(&self, ty: PersonType) -> Option<usize> {
        self.persons().iter().position(|p| p.person_type() == ty)
    }

    /// provided: 按支援卡 `card_id` 找该卡人头在 `persons` 里的下标（[`Game::deck_index_of`] 的逆向）
    ///
    /// 卡组下标与人头下标**不保证相等**（非卡人头可以插在卡片之间），
    /// 故跨容器一律按卡 ID 反查，不按下标对位。
    fn person_index_of_card(&self, card_id: u32) -> Option<usize> {
        self.persons().iter().position(|p| p.card_id() == Some(card_id))
    }

    /// provided: 人头下标 → 卡组下标
    ///
    /// `persons` 与 `deck` 是两个平行容器，下标**不保证**一一对应
    /// （剧本友人卡可能推迟加入）。需要访问 `deck[..]` 时一律走本方法反查。
    ///
    /// 前置条件「`deck` 内 `card_id` 唯一」由构造期保证：`BaseGame::new` 按 `card_id`
    /// 判重并拒绝重复卡组，故这里不会出现「重复卡静默命中第一张」。
    fn deck_index_of(&self, person_index: usize) -> Option<usize> {
        let cid = self.persons().get(person_index)?.card_id()?;
        self.deck().iter().position(|card| card.card_id == cid)
    }
    /// provided: 统计一个训练位上吃人数加成的人头数（按类型排除理事长与记者）
    ///
    /// 分身不新建人头而是再放占位，因此**按占位逐个计数，不去重**。
    /// 负数（「不出现」哨兵）与越界下标一律不计。
    fn count_training_persons(&self, train: usize) -> usize {
        let Some(dist) = self.distribution().get(train) else {
            return 0;
        };
        let persons = self.persons();
        dist.iter()
            .filter(|&&p| p >= 0)
            .filter_map(|&p| persons.get(p as usize))
            .filter(|p| !matches!(p.person_type(), PersonType::Yayoi | PersonType::Reporter))
            .count()
    }
    /// 计算来自支援卡的训练 buff（可 override）
    fn calc_training_buff(&self, train: usize) -> Result<CardTrainingEffect> {
        self.default_calc_training_buff(train)
    }

    /// calc_training_buff 的默认实现（供 override 后回 call）
    fn default_calc_training_buff(&self, train: usize) -> Result<CardTrainingEffect> {
        let mut ret = CardTrainingEffect::default();
        if train >= 5 {
            return Err(anyhow!("训练类型错误: {train}"));
        }
        // 防御：distribution 未初始化时返回空 buff
        let train_dist = self.distribution().get(train);
        if let Some(indices) = train_dist {
            for index in indices {
                // 负数为分身占位或空位
                if *index < 0 {
                    continue;
                }
                let person_index = *index as usize;
                // 人头下标 ≠ 卡组下标，必须按 card_id 反查；
                // 无卡人头（理事长 / 记者 / NPC）不贡献任何卡加成
                let Some(deck_index) = self.deck_index_of(person_index) else {
                    continue;
                };
                let card = &self.deck()[deck_index];
                let mut effect = card.calc_training_effect(self, train as i32);
                // 闪彩判定用人头下标，不能用卡组下标
                if !self.is_shining_at(person_index, train) {
                    effect.youqing = 0.0;
                }
                ret = ret.add(&effect);
            }
        }
        Ok(ret)
    }

    /// 计算训练属性（可 override）
    fn calc_training_value(&self, buffs: &CardTrainingEffect, train: usize) -> Result<ActionValue> {
        self.default_calc_training_value(buffs, train)
    }
    /// provided: 计算训练属性（训练值公式）
    fn default_calc_training_value(&self, buffs: &CardTrainingEffect, train: usize) -> Result<ActionValue> {
        let train_level = self.train_level(train) - 1; // 返回1-5处理成0-4
        if train >= 5 {
            return Err(anyhow!("训练类型错误: {train}"));
        }
        // 人数（含 NPC 与分身占位，排除理事长和记者）
        let person_count = self.count_training_persons(train);
        // 基础值
        let basic_value = &self.training_basic_table()[train][train_level];
        let basic_motivation = ((self.uma().motivation - 3) * 10) as f32;
        // 成长率
        let b = &self.uma().five_status_bonus;
        let status_bonus = [b[0], b[1], b[2], b[3], b[4], 0];
        let mut ret = ActionValue::default();
        // 副属性
        for ((slot, basic), bonus) in ret.status_pt.iter_mut().zip(basic_value).zip(&buffs.bonus) {
            if *basic > 0 {
                *slot = basic + bonus;
            }
        }
        ret.vital = basic_value[6];
        // 直接计算。假设 buffs 里已经算好中间加成
        for i in 0..6 {
            if basic_value[i] > 0 {
                let real_value = ret.status_pt[i] as f32
                    * (1.0 + 0.01 * buffs.youqing)
                    * (1.0 + 0.01 * basic_motivation * (1.0 + 0.01 * buffs.ganjing as f32))
                    * (1.0 + 0.01 * buffs.xunlian as f32)
                    * (1.0 + 0.05 * person_count as f32)
                    * (1.0 + 0.01 * status_bonus[i] as f32);
                ret.status_pt[i] = real_value.floor() as i32;
            }
        }
        // 智力回体
        if train == 4 && buffs.youqing > 0.0 {
            ret.vital += buffs.wiz_vital_bonus;
        }
        // 体力消耗降低
        if ret.vital < 0 {
            ret.vital = (ret.vital as f32 * (1.0 - 0.01 * buffs.vital_cost_drop)) as i32;
        }
        Ok(ret)
    }

    /// 粗略拟合的训练失败率，二次函数 A*(x0-x)^2+B*(x0-x)
    fn calc_training_failure_rate(&self, buffs: &CardTrainingEffect, train: usize) -> Result<f32> {
        let x0 = Data::get()?.consts.training_vital_threshold[train][self.train_level(train) - 1];
        let vital = self.uma().vital as f32;
        // 失败率修正
        let bias = if self.uma().flags.good_trainer {
            -2.0
        } else if self.uma().flags.bad_trainer {
            2.0
        } else {
            0.0
        };
        // 原始失败率最大99%（输入均为整数转来的有限值，不存在 NaN 口径差异）
        let mut f = if vital < x0 {
            (100.0 - vital) * (x0 - vital) / 40.0
        } else {
            0.0
        }
        .clamp(0.0, 99.0);
        // 如果有不擅长练习，失败率可能达到100%
        f = (f * (100.0 - buffs.fail_rate_drop) / 100.0 + bias).clamp(0.0, 100.0);
        Ok(f)
    }
}

/// 决策侧契约：训练员在动作与事件选项间选择
pub trait Trainer<G: Game> {
    /// 选择动作
    fn select_action(&self, game: &G, actions: &[<G as Game>::Action], rng: &mut StdRng) -> Result<usize>;
    /// 选择事件选项（旧接口，保留向后兼容）
    fn select_choice(&self, game: &G, choices: &[Vec<EventChoice>], rng: &mut StdRng) -> Result<usize>;
    /// 选择事件选项（携带完整 EventData 的新接口；默认回退 select_choice）
    fn select_event_choice(
        &self,
        game: &G,
        _event: &EventData,
        choices: &[Vec<EventChoice>],
        rng: &mut StdRng,
    ) -> Result<usize> {
        self.select_choice(game, choices, rng)
    }
}

/// 5 元素 i32 权重加权采样，返回桶下标
///
/// 与 `rand` 整数 [`WeightedIndex`](rand_distr::weighted::WeightedIndex) 抽样序列
/// **逐位一致**：内部走同一条 `Uniform::new(0, total).sample(rng)`（Lemire 无偏
/// 路径，rng 消费相同），桶归属用线性前缀和替代 `partition_point`（取首个累积
/// 权重 > 采样值的下标，与 rand 的 `cumulative_weights.partition_point(|w| w <=
/// chosen)` 同语义，末桶由 `chosen < total` 恒真兜底）。
///
/// 与 `WeightedIndex` 的差异仅在性能面：5 元素栈数组零堆分配。
fn sample_bucket(weights: &[i32; 5], rng: &mut impl Rng) -> Result<usize> {
    let uniform = Uniform::new(0, weights.iter().sum::<i32>())?;
    let chosen = uniform.sample(rng);
    let mut acc = 0;
    for (i, &w) in weights.iter().enumerate() {
        acc += w;
        if chosen < acc {
            return Ok(i);
        }
    }
    Ok(weights.len() - 1) // 兜底：chosen < total 恒成立，末桶必然命中
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand_distr::weighted::WeightedIndex;

    use super::*;

    /// 零分配分桶采样与 rand 整数 `WeightedIndex` 逐位等价
    ///
    /// 覆盖 5 个权重位的典型组合，同种子喂给新旧两条采样路径各 5 万次，
    /// 输出必须逐次一致——守住「替换后模拟数值逐位不变」这个前提。
    #[test]
    fn test_sample_bucket_matches_weighted_index() -> Result<()> {
        let cases: &[[i32; 5]] = &[
            [100, 100, 100, 100, 100], // 无得意率加权（train_type > 4 或 deyilv=0）
            [100, 150, 100, 100, 100], // 得意率 +50，加权位 1
            [250, 100, 100, 100, 100], // 得意率 +150，加权位 0（首位）
            [100, 100, 100, 100, 350], // 加权位 4（末桶）
            [100, 100, 30, 100, 100],  // 低权重位 2（<100）
            [140, 100, 100, 60, 100],  // 双位异常权重（防御性覆盖）
            [100, 100, 100, 100, 100],
        ];
        for (ci, weights) in cases.iter().enumerate() {
            let mut rng_new = StdRng::seed_from_u64(42);
            let mut rng_old = rng_new.clone();
            let dist = WeightedIndex::new(weights).unwrap();
            let mut matched = 0usize;
            for _ in 0..50_000 {
                let new_bucket = sample_bucket(weights, &mut rng_new)?;
                let old_bucket = dist.sample(&mut rng_old);
                matched += usize::from(new_bucket == old_bucket);
            }
            assert!(
                matched == 50_000,
                "case {ci}: {weights:?} 5 万次采样逐位一致 ({matched}/50000)"
            );
            println!("case {ci}: {weights:?} 采样逐位一致 (50000/50000)");
        }
        Ok(())
    }
}
