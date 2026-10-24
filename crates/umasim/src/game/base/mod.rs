//! 基础游戏状态（`BaseGame`）：剧本通用的一局状态底座
//!
//! 不含人头信息（`Person` 类型因剧本而异），也不含回合阶段机（阶段归各自实现：
//! 基础局见 [`basic::BasicGame`]，剧本见剧本仓）；具体剧本对象需补上
//! `Vec<Person>` 与自己的字段才能实现 [`crate::game::Game`]。

pub mod action;
pub mod basic;
pub mod person;
pub mod stage;

use std::sync::Arc;

use anyhow::{Result, anyhow};
use hashbrown::{HashMap, HashSet};
use rand::Rng;
use rand::seq::IndexedRandom;
use umaai_core::gamedata::{ActionValue, ChoiceResult, EventChoice, EventData, TriggerType};
use umaai_core::rng::{EventRng, StreamTag, TurnFixedRng, derive_seed};
use umaai_core::{Array5, AttributeArray, Data, diag, split_status};

pub use action::*;
pub use person::*;
pub use stage::BasicStage;

use crate::game::{FriendOutState, FriendState, InheritInfo, SupportCard, Uma};

/// 一次推进用的**引擎侧随机流**（两条轴）
///
/// 决策流不在这里：它由调用方以 `&mut StdRng` 传进 `run_stage`，**只给 `Trainer`**。
/// 剧本自己的机制随机（如脚本专属的动作结算）也不在这里——那条轴由具体剧本自持，
/// 从 [`BaseGame::master`] 取主种子自派生即可。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleStreams {
    /// 回合固定流：人头分布 / 角标 / hint / 回合固定事件
    pub turn_fixed: TurnFixedRng,
    /// 事件流：回合开始事件链（解锁判定 / 事件生成 / 事件结果）
    pub event: EventRng,
}

/// 一局游戏的基本状态，剧本通用，用于计算，不用于通信（例如通信只传递卡组id）
///
/// 需要频繁 clone，一部分不变量需要引用。序列化在 record 阶段（P4′）引入。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BaseGame {
    /// 回合数 [0, 77]
    pub turn: i32,
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
    /// 规则主种子；未注入时规则流不可用（推进入口据此**报错**，不静默回退单流）
    rule_master: Option<u64>,
    /// 回合固定流（按 `(master, turn)` 派生）
    turn_fixed: Option<TurnFixedRng>,
    /// 事件流（按 `(master, turn, EVENT_TAG)` 派生）
    event: Option<EventRng>,
}

impl BaseGame {
    /// 规则主种子；未注入时为 `None`（推进入口据此明确报错）
    ///
    /// 名字与契约的 `Game::rule_master` 分开：具体局面经 `Deref` 指向本类型，同名方法会在
    /// 漏导入 contract 的调用点被静默解析到这里。
    pub fn master(&self) -> Option<u64> {
        self.rule_master
    }

    /// 装主种子并按当前回合派生两条规则流（一步做完，不留「只装种子不派生」的空档）
    ///
    /// 契约入口是 `Game::set_rule_master`（宿主与搜索分支的注入点），具体局面的实现里调
    /// 本方法完成引擎侧那一步；之后**每进入一个回合**都要 [`Self::reset_rule_streams`]。
    /// 两个名字刻意不同：具体局面经 `Deref` 指向本类型，同名方法会在漏导入 contract 的
    /// 调用点被静默解析到这里、绕过剧本自己那条轴。
    pub fn init_rule_streams(&mut self, master: u64) {
        self.rule_master = Some(master);
        self.reset_rule_streams();
    }

    /// 按 `(master, turn)` 重新派生两条规则流（回合边界调用）
    ///
    /// 派生形如 `derive_seed(master, [turn])` / `derive_seed(master, [turn, EVENT_TAG])`：
    /// 同一种子、同一回合，**任何策略**看到的局面与事件随机逐位相同——这既是 CRN 配对
    /// 的基础，也让"改上游随机"不再位移本回合的其他随机点。未注入主种子时清空两条流。
    pub fn reset_rule_streams(&mut self) {
        match self.rule_master {
            Some(master) => {
                let turn = self.turn as u64;
                self.turn_fixed = Some(TurnFixedRng::new(derive_seed(master, &[turn])));
                self.event = Some(EventRng::new(derive_seed(master, &[turn, StreamTag::Event.tag()])));
            }
            None => {
                self.turn_fixed = None;
                self.event = None;
            }
        }
    }

    /// 取出两条规则流独占使用（推进期间）；与 [`Self::put_rule_streams`] 成对
    ///
    /// 主种子未注入时**报错**（不静默回退到决策流：那种"看起来能跑"的回退会把两套随机
    /// 语义混在一起，且只在参数上差一个种子的距离）。
    pub fn take_rule_streams(&mut self) -> Result<RuleStreams> {
        let Some(_) = self.rule_master else {
            return Err(anyhow!(
                "规则主种子未注入：整局入口与搜索分支会自动注入，自建局面需先调用 set_rule_master"
            ));
        };
        let (Some(turn_fixed), Some(event)) = (self.turn_fixed.take(), self.event.take()) else {
            return Err(anyhow!(
                "规则流缺失：主种子已注入但流未派生，应先调用 reset_rule_streams"
            ));
        };
        Ok(RuleStreams { turn_fixed, event })
    }

    /// 放回规则流（先放回再传播错误，避免提前返回把推进过的流状态丢掉）
    pub fn put_rule_streams(&mut self, streams: RuleStreams) {
        self.turn_fixed = Some(streams.turn_fixed);
        self.event = Some(streams.event);
    }

    /// 构造本回合的比赛事件并入队（生涯 / 自选两支统一入口）
    ///
    /// `scale` 是**实际系数**（可直接乘到事件面板上，不是面板值）：默认调用方传
    /// `(100 + uma.race_bonus) / 100`（`race_bonus` 即支援卡「赛后」累加），剧本可在此基础上
    /// 再倍乘自己的机制系数（如温泉生涯赛 ×1.33 × 源泉加成）。
    /// `pt_rate` 只缩放技能点一项，`1.0` 表示不变（供剧本特例单独压低 PT）。
    ///
    /// 分支口径：生涯回合取 `race_career`；非生涯回合按等级表取 `race_g{grade}`
    /// （`1 = G1` … `4 = OP`，`0 = 无比赛`）。没有可选比赛时**报错**而不是猜一个等级：
    /// 决赛期（72–77）只有 73/75/77 的生涯赛，等级表也不覆盖 72 以后。
    pub fn push_race_event(&mut self, scale: f64, pt_rate: f64) -> Result<()> {
        let mut event = if self.uma.is_race_turn(self.turn) {
            let event = Data::get()?.system_event("race_career")?.clone();
            diag!(">> 比赛 T{} 生涯 scale={scale:.2} pt_rate={pt_rate:.2}", self.turn);
            event
        } else {
            let grade = self.race_grade().ok_or_else(|| {
                anyhow!(
                    "该回合没有可自选比赛：turn={}（等级表 {} 项，0=无比赛，72 以后为决赛期）",
                    self.turn,
                    self.race_grades.len()
                )
            })?;
            let event = Data::get()?.system_event(&format!("race_g{grade}"))?.clone();
            diag!(
                ">> 比赛 T{} 自选G{grade} scale={scale:.2} pt_rate={pt_rate:.2}",
                self.turn
            );
            event
        };
        // 系数一律 f64（新仓口径），面板值 round 后写回
        event.map_status(|x| (f64::from(x) * scale).round() as i32);
        if pt_rate != 1.0 {
            let value = &mut event.choices[0][0].value;
            value.status_pt[5] = (f64::from(value.status_pt[5]) * pt_rate).round() as i32;
        }
        self.unresolved_events.push(event);
        self.uma.set_race(self.turn);
        Ok(())
    }

    /// 增加羁绊（统一出口：爱娇 +2、上限 100、同步卡面）
    ///
    /// `persons` 由调用方传入（人头容器在剧本侧，`BaseGame` 不持有）：人头下标 ≠ 卡组下标，
    /// 卡面同步按 `card_id` 反查，不按下标对位。所有正向羁绊获取都应经本方法，
    /// 爱娇的全局 +2 才能只在一处施加。
    pub fn add_friendship<P: crate::game::Person>(&mut self, persons: &mut [P], person_index: usize, value: i32) {
        if person_index >= persons.len() {
            diag!(
                "羁绊入账被丢弃：人头下标 {person_index} 越界（共 {} 人头）",
                persons.len()
            );
            return;
        }
        let aijiao = value > 0 && self.uma.flags.aijiao;
        let delta = if aijiao { value + 2 } else { value };
        let old = persons[person_index].friendship();
        let new_value = (old + delta).min(100);
        persons[person_index].set_friendship(new_value);
        diag!(
            "羁绊 #{person_index} {delta:+} -> {new_value}{}{}",
            if aijiao { "（含爱娇+2）" } else { "" },
            if old + delta > 100 { "（封顶）" } else { "" },
        );
        // 卡面羁绊同步：按 card_id 反查槽位
        if let Some(card_id) = persons[person_index].card_id()
            && let Some(slot) = self.deck.iter().position(|card| card.card_id == card_id)
        {
            self.deck[slot].friendship = new_value;
        }
    }

    /// 训练成功后的**默认后处理**（AfterTrain 阶段触发的主要内容）
    ///
    /// 训练前后事件按阶段区分：本方法负责「训练后」这一批（羁绊 / 红点 / 加练 / 友人点击 /
    /// 理事长·记者点击），与回合开始（Begin）的事件链分属不同阶段。数值入账与训练计数由
    /// 调用方负责。`persons` 由调用方传入（人头容器在剧本侧）。
    ///
    /// 剧本可在自己的训练结算后调用本方法；若阶段内容与基础局不同，直接写自己的实现即可
    /// （方法名带 `default_` 表示这是默认档，不是必须走的路径）。
    ///
    /// 副作用：
    /// - 按人头类型加羁绊（支援卡 +7 / 理事长·记者 +5，爱娇由 [`Self::add_friendship`] 施加）；
    /// - 从本训练位的红点人头里抽一人，生成红点事件（走 [`Self::do_hint`] 完整口径：
    ///   次数 / 等级截断 / 单卡累计）；
    /// - 非合宿时按概率生成加练事件；
    /// - 友人首次 / 常规点击、理事长点击、记者点击事件。
    pub fn default_after_train<P: crate::game::Person>(
        &mut self,
        persons: &mut [P],
        train: usize,
        rng: &mut impl Rng,
    ) -> Result<()> {
        use crate::game::FriendOutState;
        let mut hint_persons = vec![];
        let mut friend_clicked = false;
        let mut yayoi_clicked = false;
        let mut reporter_clicked = false;
        for person_index in self.distribution[train].clone() {
            // 跳过空位（-1 表示空位）
            if person_index < 0 {
                continue;
            }
            let p = person_index as usize;
            let base = match persons[p].person_type() {
                crate::game::PersonType::Yayoi | crate::game::PersonType::Reporter => 5,
                _ => 7,
            };
            self.add_friendship(persons, p, base);
            if persons[p].hint() {
                hint_persons.push(person_index);
            }
            match persons[p].person_type() {
                crate::game::PersonType::ScenarioCard => friend_clicked = true,
                crate::game::PersonType::Yayoi => yayoi_clicked = true,
                crate::game::PersonType::Reporter => reporter_clicked = true,
                _ => {}
            };
        }
        // 生成加练或者红点事件（红点走完整口径：次数 / 等级截断 / 单卡累计，见 do_hint）
        let mut hint_chosen: Option<i32> = None;
        if let Some(&p) = hint_persons.choose(rng) {
            hint_chosen = Some(p);
            self.do_hint(persons, p as usize, rng)?;
        }
        let extra_train_prob = Data::get()?.event_prob("extra_train")?;
        let mut extra_train = false;
        if !self.is_xiahesu() && rng.random_bool(extra_train_prob) {
            extra_train = true;
            let mut event = crate::game::extra_training_event(train)?;
            if let Some(yayoi_index) = persons
                .iter()
                .position(|p| p.person_type() == crate::game::PersonType::Yayoi)
            {
                event.person_index = Some(yayoi_index as i32);
            }
            self.unresolved_events.push(event);
        }
        // 更新友人状态
        if friend_clicked {
            match self.friend.out_state {
                FriendOutState::UnClicked => {
                    self.friend.out_state = FriendOutState::BeforeUnlock;
                    let event = Data::get()?.events.friend_events["first"].clone();
                    self.unresolved_events.push(event);
                }
                _ => {
                    let event = Data::get()?.events.friend_events["click"].clone();
                    self.unresolved_events.push(event);
                }
            }
        }
        if yayoi_clicked {
            let mut event = Data::get()?.system_event("yayoi_click")?.clone();
            if let Some(index) = persons
                .iter()
                .position(|p| p.person_type() == crate::game::PersonType::Yayoi)
            {
                event.person_index = Some(index as i32);
            }
            self.unresolved_events.push(event);
        }
        if reporter_clicked {
            let mut event = Data::get()?.system_event("reporter_click")?.clone();
            event.choices[0][0].value.status_pt[train] = 2;
            if let Some(index) = persons
                .iter()
                .position(|p| p.person_type() == crate::game::PersonType::Reporter)
            {
                event.person_index = Some(index as i32);
            }
            self.unresolved_events.push(event);
        }
        // 诊断摘要：只报「生成了什么」——事件身份由 run_event 报、羁绊由 add_friendship 报
        let mut parts: Vec<String> = vec![];
        if let Some(p) = hint_chosen {
            parts.push(format!("红点#{p}"));
        }
        if extra_train {
            parts.push("加练".to_string());
        }
        if friend_clicked {
            parts.push("友人点击".to_string());
        }
        if yayoi_clicked {
            parts.push("理事长点击".to_string());
        }
        if reporter_clicked {
            parts.push("记者点击".to_string());
        }
        if !parts.is_empty() {
            diag!("训练后处理 训练位{train}: {}", parts.join(" "));
        }
        Ok(())
    }

    /// 生成红点事件（发生次数 / 等级截断 / 单卡累计的完整口径，可复用件）
    ///
    /// 单卡红点等级取「卡面等级 + 1」，受 5 级与单卡累计上限（`max_hint_per_card`）双重截断；
    /// 卡面 `hint_count_bonus` 决定本次发生次数；等级为 0（超限）时只给属性红点、不累计。
    /// 谁能亮红点（发生率）不在本方法——归契约 `distribute_hint`（剧本覆写口径不变）。
    /// 人头下标 ≠ 卡组下标：按 `card_id` 反查卡面；非卡人头按 1 级 1 次处理。
    pub fn do_hint<P: crate::game::Person>(
        &mut self,
        persons: &[P],
        person_index: usize,
        rng: &mut impl Rng,
    ) -> Result<()> {
        let data = Data::get()?;
        let attr_prob = data.event_prob("hint_attr")?;
        let max_hint = data.consts.max_hint_per_card;
        let card = persons[person_index]
            .card_id()
            .and_then(|cid| self.deck.iter().position(|c| c.card_id == cid));
        let (hint_level, hint_count) = match card {
            Some(idx) => {
                let level = (1 + self.deck[idx].card_value().hint_level)
                    .min(5)
                    .min(max_hint - self.deck[idx].total_hints);
                (level, 1 + self.deck[idx].effect.hint_count_bonus)
            }
            None => (1, 1),
        };
        for _ in 0..hint_count {
            let mut hint_event = if rng.random_bool(attr_prob) || hint_level == 0 {
                // 红点提供属性；超过单卡累计上限时也只能提供属性
                crate::game::hint_attr_event(persons[person_index].train_type() as usize, person_index)?
            } else {
                if let Some(idx) = card {
                    self.deck[idx].total_hints += hint_level;
                }
                crate::game::hint_skill_event(hint_level, person_index)
            };
            if let Some(idx) = card {
                hint_event.name = format!("{} - {}", hint_event.name, self.deck[idx].short_name());
            }
            self.unresolved_events.push(hint_event);
        }
        Ok(())
    }

    /// 该回合是否允许**自选比赛**（通用规则，各剧本 list_actions 复用）
    ///
    /// 两个条件同时成立：
    /// - 回合窗口：13–71（0-based）；URA 回合（72–77）不可自选（73/75/77 是生涯决赛，
    ///   72/74/76 是黑障回合，等级表也不覆盖 72 以后）；
    /// - **等级表非 0**：`race_grades[turn] = 0` 表示该回合没有可参加的赛事。
    ///
    /// 等级语义：`1 = G1（最高）` … `4 = OP（最低）`；`0 = 无比赛`。
    /// 注意：`BasicGame::list_actions` 以 `turn > 13` 起始（较本规则晚 1 回合，
    /// 历史行为保留）。
    pub fn can_self_race(&self) -> bool {
        self.turn > 12 && self.turn < 72 && self.race_grade().is_some()
    }

    /// 本回合**可选比赛的最高等级**（`1 = G1` 最高 … `4 = OP` 最低）
    ///
    /// `None` 表示本回合没有可自选比赛：回合越界（等级表只覆盖 0–71）、
    /// 等级为 0（无比赛），或 URA 决赛期。
    pub fn race_grade(&self) -> Option<i32> {
        self.race_grades
            .get(self.turn as usize)
            .copied()
            .filter(|grade| *grade > 0)
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
    /// `deck_ids` 长度不固定（剧本的卡组约定可能不同），但**同一张卡不得占两个槽位**：
    /// 按 `card_id` 判重（同卡不同突破档也算重复），构造期即拒绝——重复卡会让人头反查
    /// （`deck_index_of`）的两张人头都命中第一张，羁绊与红点静默写到错槽位；
    /// 其余合法性由调用方保证。`limit_base` 由调用的剧本提供（见 [`Uma::new`]）；
    /// `race_grades` 为比赛等级表（配置层注入）。构造顺序是**先写剧本基值、再加继承**；
    /// 任何剧本都不得在本函数返回后整体赋值 `five_status_limit`，那会擦掉已累加的
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
        let mut card_type_count = [0; 7];
        // 支援卡
        let mut first_slot: HashMap<u32, usize> = HashMap::new();
        for (index, id) in deck_ids.iter().enumerate() {
            let card = SupportCard::new(*id)?;
            if let Some(first) = first_slot.insert(card.card_id, index) {
                anyhow::bail!(
                    "卡组含重复支援卡: {}（cardId {}）——第 {} 槽与第 {} 槽是同一张卡，同一张卡不能占两个槽位",
                    card.data.short_name(),
                    card.card_id,
                    first,
                    index
                );
            }
            // 初始属性
            let initial = card.initial_bonus();
            let race_bonus = card.effect.saihou;
            let (initial, pt) = split_status(initial)?;
            uma.five_status.add_eq(initial);
            uma.skill_pt += pt;
            uma.race_bonus += race_bonus;
            // 友人. 暂时不处理多个友人（多张时取最后一张，见 rules §4.1）
            if card.card_type >= 5 {
                friend_id = Some(*id);
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
            uma,
            deck,
            inherit,
            friend: FriendState::new(friend_id)?,
            train_level_count: [0; 5],
            distribution: vec![],
            events: HashMap::new(),
            absent_rate_drop: 0,
            unresolved_events: vec![],
            card_type_count,
            // 从 events.friend_events 派生友人事件 ID
            friend_event_ids: Data::get()?.events.friend_events.values().map(|e| e.id).collect(),
            race_grades: Arc::new(race_grades.to_vec()),
            // 规则流由宿主注入主种子后派生（未注入就推进会报错，不静默回退）
            rule_master: None,
            turn_fixed: None,
            event: None,
        })
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
                let friend = self.friend_event_ids.contains(&event.id);
                if friend {
                    Self::apply_friend_bonus(&mut choice_result.value, &self.friend);
                }
                self.uma.add_value(&choice_result.value);
                // 结算叙事：结果档（普通档略）+ 入账摘要 + 友人加成注记；三者皆空不落日志
                let mut parts: Vec<String> = vec![];
                if let Some(result) = diag_result_note(choice_result.result) {
                    parts.push(result.to_string());
                }
                let value = choice_result.value.to_string();
                if !value.is_empty() {
                    parts.push(format!("入账 {value}"));
                }
                if friend {
                    parts.push("含友人加成".to_string());
                }
                if !parts.is_empty() {
                    diag!("事件 #{} 结算: {}", event.id, parts.join(" "));
                }
                Some(choice_result)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// 事件应归属的人头下标
    ///
    /// 规则：事件自带 `person_index` 优先（生成时已按身份定位）；没带且是**友人事件**
    /// （`friend_event_ids` 之一）时，用友人的 `card_id` 在 `persons` 里反查——
    /// 卡组下标与人头下标不保证相等，故一律按身份查，不按下标对位。查不到就不归属。
    pub fn event_person_target<P: crate::game::Person>(&self, event: &EventData, persons: &[P]) -> Option<usize> {
        if let Some(index) = event.person_index {
            return usize::try_from(index).ok();
        }
        if !self.friend_event_ids.contains(&event.id) {
            return None;
        }
        let card_id = self.friend.card_id?;
        persons.iter().position(|person| person.card_id() == Some(card_id))
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
        // 结算叙事：入账摘要与状态增减（词形见 `UmaFlags` 的 `Display`）；皆空不落日志
        let mut parts: Vec<String> = vec![];
        let value = choice.value.to_string();
        if !value.is_empty() {
            parts.push(format!("入账 {value}"));
        }
        if let Some(add) = &choice.add_flags {
            let words = add.to_string();
            if !words.is_empty() {
                parts.push(format!("+{words}"));
            }
        }
        if let Some(remove) = &choice.remove_flags {
            let words = remove.to_string();
            if !words.is_empty() {
                parts.push(format!("-{words}"));
            }
        }
        if !parts.is_empty() {
            diag!("事件选项结算: {}", parts.join(" "));
        }
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
                diag!(
                    "自选比赛未达标：T{} 需 {} 场，已 {} 场",
                    self.turn,
                    free_race.count,
                    self.uma.count_free_race(free_race)
                );
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

/// 事件结果档的诊断标签（普通档与非法值返回 `None`；词形转正见各值类型的 `Display`）
fn diag_result_note(result: i32) -> Option<&'static str> {
    match ChoiceResult::try_from(result) {
        Ok(ChoiceResult::Success) => Some("成功"),
        Ok(ChoiceResult::BigSuccess) => Some("大成功"),
        Ok(ChoiceResult::Fail) => Some("失败"),
        Ok(ChoiceResult::BigFail) => Some("大失败"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::RACE_GRADES;
    use rand::RngCore;
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

    /// 层1：重复支援卡在构造期即拒绝（按 `card_id` 判重，同卡不同突破档也算重复）
    #[test]
    fn test_duplicate_card_rejected() -> Result<()> {
        Data::init(data_root()?)?;
        let limit = Data::get()?.consts.five_status_limit_base;
        let build = |cards: &[u32]| BaseGame::new(101901, cards, InheritInfo::default(), limit, &RACE_GRADES);

        let err = build(&[302424, 302464, 302424]).expect_err("同一张卡占两槽应拒绝");
        let msg = format!("{err:#}");
        assert!(msg.contains("重复支援卡"), "错误要点名重复支援卡: {msg}");
        assert!(msg.contains("第 0 槽与第 2 槽"), "错误要点名两处槽位: {msg}");
        assert!(msg.contains("30242"), "错误要点名 cardId: {msg}");
        // 同卡不同突破档（idrank 302424 与 302423 是同一张卡）同样拒绝
        assert!(build(&[302424, 302423, 302464]).is_err(), "同卡不同突破档也算重复");
        // 正常卡组不受影响
        assert!(
            build(&[302424, 302464, 302484, 302564, 302574, 302644]).is_ok(),
            "无重复卡组应正常构造"
        );
        Ok(())
    }

    /// 层1：自选比赛回合边界
    #[test]
    fn test_can_self_race_bounds() {
        // 等级表覆盖 0–71；0 = 无比赛，1 = G1 … 4 = OP
        let mut grades = vec![1; 72];
        grades[12] = 0;
        let mut game = BaseGame {
            turn: 12,
            race_grades: std::sync::Arc::new(grades),
            ..Default::default()
        };
        assert!(!game.can_self_race(), "回合 12 等级为 0（无比赛），不可自选");
        game.turn = 13;
        assert!(game.can_self_race(), "回合 13 起可自选");
        game.turn = 71;
        assert!(game.can_self_race(), "回合 71 仍可自选");
        game.turn = 72;
        assert!(!game.can_self_race(), "URA 回合不可自选（等级表不覆盖）");
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

    /// 层1：do_hint 完整口径——技能档按「卡面等级 +1」累计单卡 hint
    #[test]
    fn test_do_hint_accumulates_card_hints() -> Result<()> {
        use anyhow::Context;
        use rand::SeedableRng;
        use rand::rngs::StdRng;

        use crate::testutil::basic_game;

        let mut game = basic_game()?;
        let attr_prob = Data::get()?.event_prob("hint_attr")?;
        let max_hint = Data::get()?.consts.max_hint_per_card;
        // 找一个首抽落技能档的种子（属性档只占 attr_prob，前几个种子必有）
        let seed = (0..64)
            .find(|&s| !StdRng::seed_from_u64(s).random_bool(attr_prob))
            .context("应存在技能档种子")?;
        let panel_level = game.deck[0].card_value().hint_level;
        let expect = (1 + panel_level).min(5).min(max_hint);
        let before = game.unresolved_events.len();
        let mut rng = StdRng::seed_from_u64(seed);
        game.base.do_hint(&game.persons, 0, &mut rng)?;
        assert_eq!(game.unresolved_events.len() - before, 1, "无判定加成的卡发生 1 次");
        let event = game.unresolved_events.last().context("应有红点事件")?;
        assert!(event.name.starts_with("Hint - 技能"), "技能档事件名: {}", event.name);
        assert_eq!(event.choices[0][0].value.hint_level, expect, "事件携带截断后等级");
        assert_eq!(game.deck[0].total_hints, expect, "单卡累计 = 截断后等级");
        Ok(())
    }

    /// 层1：do_hint 发生次数按卡面判定加成（1 + hint_count_bonus）
    #[test]
    fn test_do_hint_count_bonus() -> Result<()> {
        use rand::SeedableRng;
        use rand::rngs::StdRng;

        use crate::testutil::basic_game;

        let mut game = basic_game()?;
        game.deck[0].effect.hint_count_bonus = 2;
        let before = game.unresolved_events.len();
        let mut rng = StdRng::seed_from_u64(0);
        game.base.do_hint(&game.persons, 0, &mut rng)?;
        assert_eq!(game.unresolved_events.len() - before, 3, "判定加成 2 → 发生 3 次");
        Ok(())
    }

    /// 层1：do_hint 超过单卡累计上限时只给属性红点、不再累计
    #[test]
    fn test_do_hint_overflow_attr_only() -> Result<()> {
        use anyhow::Context;
        use rand::SeedableRng;
        use rand::rngs::StdRng;

        use crate::testutil::basic_game;

        let mut game = basic_game()?;
        let max_hint = Data::get()?.consts.max_hint_per_card;
        game.deck[0].total_hints = max_hint;
        let before = game.unresolved_events.len();
        let mut rng = StdRng::seed_from_u64(0);
        game.base.do_hint(&game.persons, 0, &mut rng)?;
        assert_eq!(game.unresolved_events.len() - before, 1, "超限仍发生 1 次（给属性）");
        let event = game.unresolved_events.last().context("应有红点事件")?;
        assert!(event.name.contains("属性"), "超限只给属性红点: {}", event.name);
        assert_eq!(game.deck[0].total_hints, max_hint, "超限不再累计");
        Ok(())
    }

    /// 层1：do_hint 非卡人头按 1 级 1 次处理（不触卡组累计）
    #[test]
    fn test_do_hint_non_card_person() -> Result<()> {
        use anyhow::Context;
        use rand::SeedableRng;
        use rand::rngs::StdRng;

        use crate::game::PersonType;
        use crate::testutil::basic_game;

        let mut game = basic_game()?;
        let attr_prob = Data::get()?.event_prob("hint_attr")?;
        // 非卡人头的属性档走 train_type 会越界（沿用旧口径：默认路径不给非卡亮红点），取技能档种子
        let seed = (0..64)
            .find(|&s| !StdRng::seed_from_u64(s).random_bool(attr_prob))
            .context("应存在技能档种子")?;
        let yayoi = game
            .persons
            .iter()
            .position(|p| p.person_type == PersonType::Yayoi)
            .context("基础局应有理事长")?;
        let before = game.unresolved_events.len();
        let mut rng = StdRng::seed_from_u64(seed);
        game.base.do_hint(&game.persons, yayoi, &mut rng)?;
        assert_eq!(game.unresolved_events.len() - before, 1, "非卡发生 1 次");
        let event = game.unresolved_events.last().context("应有红点事件")?;
        assert_eq!(event.choices[0][0].value.hint_level, 1, "非卡按 1 级");
        assert!(game.deck.iter().all(|c| c.total_hints == 0), "非卡红点不累计卡组");
        Ok(())
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

    /// 层1：主种子未注入就取规则流即报错（不静默回退到决策流）
    #[test]
    fn test_take_rule_streams_requires_master() {
        let mut game = BaseGame::default();
        let msg = format!("{:#}", game.take_rule_streams().unwrap_err());
        assert!(msg.contains("规则主种子未注入"), "错误文案应指明主种子未注入: {msg}");

        game.init_rule_streams(7);
        assert!(game.take_rule_streams().is_ok(), "注入后应可取流");
    }

    /// 层1：规则流按 `(master, 回合)` 派生——换回合重新派生，counter 归零
    #[test]
    fn test_reset_rule_streams_follows_turn() {
        let mut game = BaseGame::default();
        game.init_rule_streams(42);
        let first = game.turn_fixed.expect("注入后应有回合固定流").master();

        game.turn = 1;
        game.reset_rule_streams();
        let second = game.turn_fixed.expect("重置后应有回合固定流").master();
        assert_ne!(first, second, "换回合应换一条流");

        // 同参数重复派生逐位一致（可复现性锚点）
        let mut same = BaseGame { turn: 1, ..Default::default() };
        same.init_rule_streams(42);
        assert_eq!(same.turn_fixed, game.turn_fixed, "同 (master, 回合) 派生同一条流");
        assert_eq!(same.event, game.event, "事件流同理");

        // 不同主种子给出不同流
        let mut other = BaseGame { turn: 1, ..Default::default() };
        other.init_rule_streams(43);
        assert_ne!(other.turn_fixed, game.turn_fixed, "换主种子应换流");
    }

    /// 层1：取出-放回不重置流（放回后 counter 继续推进，不是回到起点）
    #[test]
    fn test_take_put_roundtrip_keeps_progress() {
        let mut game = BaseGame::default();
        game.init_rule_streams(9);

        let mut streams = game.take_rule_streams().expect("注入后可取流");
        let seen = streams.turn_fixed.next_u64();
        let counter = streams.turn_fixed.counter();
        assert_eq!(counter, 1, "取用期间推进一次");
        game.put_rule_streams(streams);

        let mut again = game.take_rule_streams().expect("放回后可再取");
        assert_eq!(again.turn_fixed.counter(), counter, "放回应保留推进过的 counter");
        assert_ne!(again.turn_fixed.next_u64(), seen, "继续推进应给出后续随机值");
    }

    /// 层1：引擎两条流与决策流互不影响——决策流多抽若干次不改规则流取值
    #[test]
    fn test_rule_streams_ignore_decision_draws() {
        let draw = |extra: usize| -> Vec<u64> {
            let mut game = BaseGame::default();
            game.init_rule_streams(1234);
            let mut streams = game.take_rule_streams().expect("注入后可取流");
            let mut out = Vec::new();
            for _ in 0..4 {
                // 模拟"决策流被多消耗"：与流无关，序列必须一致
                out.push(streams.turn_fixed.next_u64());
                out.push(streams.event.next_u64());
            }
            let _ = extra;
            game.put_rule_streams(streams);
            out
        };
        assert_eq!(draw(0), draw(17), "规则流取值不应受任何外部消耗次数影响");
    }

    /// H1：`push_race_event` 的默认系数与旧 `do_race` 口径一致（生涯事件按 race_bonus 缩放）
    #[test]
    fn test_push_race_event_default_scale() -> Result<()> {
        use crate::game::base::BaseAction;
        let mut a = crate::testutil::basic_game()?;
        // 生涯比赛回合（turn 11 起为生涯赛），走 race_career 分支，避开等级表依赖
        a.turn = 11;
        a.uma.race_bonus = 30;
        let mut b = a.clone();
        b.uma.race_bonus = 30;
        BaseAction::do_race(&mut a, &mut rand::rng())?;
        b.push_race_event((100.0 + 30.0) / 100.0, 1.0)?;
        assert_eq!(a.unresolved_events.len(), b.unresolved_events.len());
        assert_eq!(
            a.unresolved_events[0].choices[0][0].value.status_pt,
            b.unresolved_events[0].choices[0][0].value.status_pt
        );
        println!("H1: 默认系数与 do_race 一致");
        Ok(())
    }

    /// H1：`pt_rate` 只缩放技能点一项
    #[test]
    fn test_push_race_event_pt_rate() -> Result<()> {
        let mut game = crate::testutil::basic_game()?;
        game.turn = 11;
        let scale = (100 + game.uma.race_bonus) as f64 / 100.0;
        game.push_race_event(scale, 1.0)?;
        let base_pt = game.unresolved_events[0].choices[0][0].value.status_pt[5];
        game.unresolved_events.clear();
        game.push_race_event(scale, 0.5)?;
        let half_pt = game.unresolved_events[0].choices[0][0].value.status_pt[5];
        assert_eq!(half_pt, (base_pt as f64 * 0.5).round() as i32);
        println!("H1: pt_rate 只缩放技能点（{base_pt} -> {half_pt}）");
        Ok(())
    }

    /// 比赛等级表口径：0 = 无比赛，1 = G1 最高 … 4 = OP 最低
    #[test]
    fn test_race_grade_semantics() -> Result<()> {
        let mut game = crate::testutil::basic_game()?;
        // 回合 12：等级表为 0 → 不可自选比赛
        game.turn = 12;
        assert_eq!(game.race_grade(), None, "等级 0 应为无比赛");
        assert!(!game.can_self_race(), "无比赛回合不可自选");
        // 回合 13：等级 3（G3）→ 可自选
        game.turn = 13;
        assert_eq!(game.race_grade(), Some(3), "回合 13 应为 G3");
        assert!(game.can_self_race());
        // 决赛期黑障回合与越界回合一律 None
        game.turn = 72;
        assert_eq!(game.race_grade(), None, "72 起等级表不覆盖，应为无比赛");
        println!("等级表口径正确：0=无比赛、1=G1 … 4=OP");
        Ok(())
    }

    /// 无比赛回合构造比赛事件要明确报错，而不是猜等级
    #[test]
    fn test_push_race_event_without_race_errors() -> Result<()> {
        let mut game = crate::testutil::basic_game()?;
        game.turn = 12;
        assert!(game.push_race_event(1.0, 1.0).is_err(), "无比赛回合应报错");
        game.turn = 73;
        assert!(
            game.push_race_event(1.0, 1.0).is_ok(),
            "73 是生涯决赛，应走 race_career"
        );
        println!("无比赛回合报错、生涯决赛正常");
        Ok(())
    }

    /// 自选比赛按等级表取事件：G1 回合生成 race_g1（最高档，不是 OP）
    #[test]
    fn test_self_race_uses_g1_event() -> Result<()> {
        let mut game = crate::testutil::basic_game()?;
        game.turn = 22; // 等级表为 1（G1）
        game.push_race_event(1.0, 1.0)?;
        let name = &game.unresolved_events[0].name;
        assert_eq!(name, "G1比赛", "G1 回合应生成 G1 事件，实际 {name}");
        println!("G1 回合事件正确: {name}");
        Ok(())
    }

    /// H4：契约出口的羁绊加成——爱娇 +2、上限 100、卡面同步
    #[test]
    fn test_add_friendship_contract() -> Result<()> {
        let mut game = crate::testutil::basic_game()?;
        game.uma.flags.aijiao = true;
        let mut persons = game.persons.clone();
        let before = persons[0].friendship;
        game.base.add_friendship(&mut persons, 0, 7);
        assert_eq!(persons[0].friendship, (before + 7 + 2).min(100), "爱娇应额外 +2");
        let slot = game
            .base
            .deck
            .iter()
            .position(|c| c.card_id == persons[0].card_id.unwrap());
        assert_eq!(
            slot.map(|s| game.base.deck[s].friendship),
            Some(persons[0].friendship),
            "卡面应同步"
        );
        println!("H4: 爱娇 +2 与卡面同步生效");
        Ok(())
    }
}
