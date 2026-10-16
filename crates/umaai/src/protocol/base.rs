//! 帧 `baseGame` 段：通用局面字段 → 局面底座
//!
//! 字段名与线上协议**逐字对应**（`camelCase` + 若干历史 snake_case 例外），未知键一律
//! 忽略（插件新增字段不得让整帧失败）；必填 / 可选的分界照抄既有 serde 契约——插件
//! 字段漂移要在解析处炸出声，不能静默按 0 参与决策。
//!
//! 转换出的 [`BaseGame`] 只覆盖通用字段：**人头构造与回合阶段归剧本侧**。阶段是剧本
//! 自定的位置标识（`Game::stage`），由剧本按帧内 `playing_state` / `source` 判定。

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use umaai_core::flags::UmaFlags;
use umaai_core::{Array5, Array6, Data, Ill};
use umasim::game::base::BaseGame;
use umasim::game::{FriendCardState, FriendOutState, FriendState, InheritInfo, Uma};

use crate::protocol::story::StoryStatus;

/// 帧里单个待决策人头的通用字段
///
/// 人头怎么映射到 [`umasim::game::Person`] 由剧本侧决定；本结构只承载线上原样字段。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BasePersonStatus {
    /// 人头类型（取值由客户端定义）
    #[serde(default)]
    pub person_type: u32,
    /// 角色 ID
    #[serde(default)]
    pub chara_id: u32,
    /// 羁绊
    #[serde(default)]
    pub friendship: i32,
    /// 是否红点
    #[serde(default)]
    pub is_hint: bool,
}

/// 事件的归属对象（由线上事件 ID 解码而来）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventOwner {
    /// 支援卡事件（归属为卡 ID）
    SupportCard(u32),
    /// 角色 / 友人事件（归属为角色 ID）
    Character(u32),
    /// 未识别的归属
    Unknown(u32),
}

/// 线上事件 ID 解码结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventRef {
    /// 归属对象
    pub owner: EventOwner,
    /// 该归属对象的事件序号（支援卡连续事件的序号即段号）
    pub seq: u32,
}

/// 解码线上事件 ID：`8 + 归属(5 位) + 序号(3 位)`
///
/// 归属首位 `3` 为支援卡（值 = 卡 ID）、首位 `0` 为角色 / 友人（值 = 角色 ID）；
/// 其余形态归 [`EventOwner::Unknown`]。真机样本（卡 30137/30242/30289/30297/30304、
/// 角色 9001/9044）与模拟表 `friend_events`（角色 9050）同法编码。
pub fn decode_event_id(id: i32) -> EventRef {
    let text = id.to_string();
    if text.len() != 9 || !text.starts_with('8') {
        return EventRef {
            owner: EventOwner::Unknown(id.max(0) as u32),
            seq: 0,
        };
    }
    let owner = text[1..6].parse::<u32>().unwrap_or(0);
    let seq = text[6..9].parse::<u32>().unwrap_or(0);
    let owner = match text.as_bytes()[1] {
        b'3' => EventOwner::SupportCard(owner),
        b'0' => EventOwner::Character(owner),
        _ => EventOwner::Unknown(owner),
    };
    EventRef { owner, seq }
}

/// 帧 `baseGame` 段 → 局面的结果
#[derive(Debug, Clone)]
pub struct FrameOutcome {
    /// 重建出的局面底座（通用字段；阶段归剧本侧，由剧本自行判定）
    pub game: BaseGame,
    /// 非致命问题（如事件数值不完整导致的事件跳过）；引擎不打印，由调用方决定告警 / 记录
    pub notes: Vec<FrameNote>,
}

/// 帧解析中的非致命问题
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameNote {
    /// 事件数值不完整，该事件未进待处理队列（帧其余部分照常生效）
    EventSkipped {
        /// 事件 ID
        id: u32,
        /// 跳过原因
        detail: String,
    },
}

/// 帧 `baseGame` 段（通用字段）
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseStatus {
    /// 剧本 ID（引擎不解释，由宿主注入的剧本档案判定是否接受）
    pub scenario_id: u32,
    /// 马娘 ID
    pub uma_id: u32,
    /// 马娘星数（随帧下发，暂不参与计算）
    #[serde(default)]
    pub uma_star: u32,
    /// 回合（0 基，0-77）
    pub turn: i32,
    /// 体力
    pub vital: i32,
    /// 最大体力
    pub max_vital: i32,
    /// 干劲 `[1, 5]`
    pub motivation: i32,
    /// 当前五维（1200 以上不减半）
    pub five_status: Array5,
    /// 五维上限
    pub five_status_limit: Array5,
    /// 剩余技能点
    pub skill_pt: i32,
    /// 已学技能分
    pub skill_score: i32,
    /// 总 Hint 等级
    pub total_hints: i32,
    /// 训练设施等级计数 `[训练 0-4]`
    pub train_level_count: Array5,
    /// PT 折算系数（随帧下发；新实现以数据表常量为准，本字段仅留档）
    #[serde(default)]
    pub pt_score_rate: f32,
    /// 训练失败率修正：正值 = 更易失败（不擅长训练），负值 = 更不易失败
    pub failure_rate_bias: i32,
    /// 是否生病（协议只给布尔，种类归 [`Ill::UNKNOWN`]）
    pub is_ill: bool,
    /// 是否切者
    #[serde(rename = "isQieZhe")]
    pub is_qiezhe: bool,
    /// 是否小切
    #[serde(default)]
    pub is_xiao_qie: bool,
    /// 是否爱娇
    #[serde(rename = "isAiJiao")]
    pub is_aijiao: bool,
    /// 心情盾（正向思考）剩余次数
    #[serde(default, rename = "PositiveThinkingCount")]
    pub positive_thinking_count: i32,
    /// 是否有休息心得
    #[serde(default)]
    pub is_refresh_mind: bool,
    /// 幸运体质次数
    #[serde(default, rename = "LuckyCount")]
    pub lucky_count: i32,
    /// 种马蓝因子数量
    #[serde(rename = "zhongMaBlueCount")]
    pub zhongma_blue_count: Array5,
    /// 是否生涯比赛状态
    pub is_racing: bool,
    /// 卡组（支援卡 ID）
    pub card_id: Vec<u32>,
    /// 人头列表
    #[serde(default)]
    pub persons: Vec<BasePersonStatus>,
    /// 人头分布 `[训练][位]`，`-1` 为不在
    #[serde(default)]
    pub person_distribution: Vec<Vec<i32>>,
    /// 是否锁定到某个训练
    pub locked_training_id: i32,
    /// 理事长羁绊（非卡人头，由剧本侧使用）
    #[serde(rename = "friendship_noncard_yayoi")]
    pub friendship_noncard_yayoi: i32,
    /// 记者羁绊（非卡人头，由剧本侧使用）
    #[serde(rename = "friendship_noncard_reporter")]
    pub friendship_noncard_reporter: i32,
    /// 友人阶段（0 未点击 / 1 已点击未出行 / 2 已出行 / 其它 = 离开）
    #[serde(rename = "friend_stage")]
    pub friend_stage: i32,
    /// 已用掉的友人出行段数
    #[serde(rename = "friend_outgoingUsed")]
    pub friend_outgoing_used: i32,
    /// 回合状态（剧本按它 dispatch 阶段）
    #[serde(rename = "playing_state")]
    pub playing_state: i32,
    /// 本回合是否已执行过指令（仅路由参考）
    #[serde(default)]
    pub is_after_training: bool,
    /// 本局是否从第 1 回合完整记录（仅路由参考）
    #[serde(default)]
    pub is_full_game: bool,
    /// 已比赛回合列表（用于还原自选比赛进度）
    #[serde(default)]
    pub race_history: Vec<i32>,
    /// 本局已发生的关键事件 story_id 列表
    #[serde(default)]
    pub key_events: Vec<i32>,
    /// 每次继承事件的属性增量（仅留档）
    #[serde(default)]
    pub inherit_gains: Vec<i32>,
    /// 本局累计花掉的技能点（解析时回加到剩余技能点，还原「本局累计获得」）
    #[serde(default)]
    pub skill_pt_spent: i32,
    /// 事件帧的选项数据；无事件时为 `None`
    #[serde(default)]
    pub story: Option<StoryStatus>,
    /// 回合阶段来源（`command` / `event` / `special`；剧本 dispatch 参考）
    #[serde(default)]
    pub source: Option<String>,
    /// 育成局号（单调递增；缺字段时调用方退化到马娘 ID）
    #[serde(default, rename = "single_mode_chara_id")]
    pub single_mode_chara_id: Option<u64>,
}

/// 通用段转换所需的宿主参数
///
/// 协议不发的量（继承的额外属性、比赛等级表）与剧本相关的量（剧本友人卡、友人首次
/// 事件 ID）都由宿主注入，本层不含任何剧本常量。
#[derive(Debug, Clone, Copy)]
pub struct BaseParams<'a> {
    /// 继承的额外属性（协议只发蓝因子，此项来自配置）
    pub extra_count: Array6,
    /// 比赛等级表（配置层注入）
    pub race_grades: &'a [i32],
    /// 剧本友人的角色 ID（0 = 本剧本没有友人卡）
    pub friend_chara_id: u32,
    /// 剧本友人首次点击事件 ID（0 = 未知，跳过该判定）
    pub friend_first_event: i32,
}

/// 从帧原文取出 `baseGame` 通用段（剧本专属段忽略）
pub fn parse_base_status(frame: &str) -> Result<BaseStatus> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Envelope {
        base_game: BaseStatus,
    }

    let envelope: Envelope = serde_json::from_str(frame).with_context(|| anyhow!("帧解析失败（baseGame 段）"))?;
    Ok(envelope.base_game)
}

impl BaseStatus {
    /// 育成局号：缺 `single_mode_chara_id` 时退化为马娘 ID
    pub fn epoch_key(&self) -> u64 {
        self.single_mode_chara_id.unwrap_or(self.uma_id as u64)
    }

    /// 帧字段 → 状态位
    ///
    /// `failureRateBias` 符号与训练失败率同源：正值 = 更易失败（不擅长），负值 = 更不易
    /// 失败（擅长）——曾按反方向解读，把擅长训练误标成不擅长，已记在 issues。
    pub fn to_flags(&self) -> UmaFlags {
        UmaFlags {
            ill: if self.is_ill { Ill::UNKNOWN } else { Ill::NONE },
            lucky_count: self.lucky_count,
            qiezhe: self.is_qiezhe,
            xiaoqie: self.is_xiao_qie,
            aijiao: self.is_aijiao,
            good_trainer: self.failure_rate_bias < 0,
            bad_trainer: self.failure_rate_bias > 0,
            positive_thinking_count: self.positive_thinking_count,
            refresh_mind: self.is_refresh_mind as i32,
            ..Default::default()
        }
    }

    /// 把帧字段写入马娘状态
    ///
    /// 只覆盖帧携带的字段，构造期得到的生涯赛程 / 赛程掩码 / 赛后加成原样保留；
    /// 技能点回加 `skillPtSpent`——协议发的是**剩余**技能点，只取剩余会让买过技能的局面
    /// 凭空丢分（搜索基线假跌，被误记成运气波动）。
    pub fn apply_to_uma(&self, uma: &mut Uma) {
        uma.vital = self.vital;
        uma.max_vital = self.max_vital;
        uma.motivation = self.motivation;
        uma.five_status = self.five_status;
        uma.five_status_limit = self.five_status_limit;
        uma.skill_pt = self.skill_pt + self.skill_pt_spent;
        uma.skill_score = self.skill_score;
        uma.total_hints = self.total_hints;
        uma.flags = self.to_flags();
        for turn in &self.race_history {
            uma.set_race(*turn);
        }
    }

    /// 帧字段 → 友人状态（按剧本友人卡定位；未带该卡时返回默认空状态）
    ///
    /// 出行段数越界（协议给出 >5）时只取前 5 段，不 panic。
    pub fn to_friend(&self, friend_chara_id: u32) -> Result<FriendState> {
        for (index, id) in self.card_id.iter().enumerate() {
            let card = umasim::game::SupportCard::new(*id)?;
            if card.card_type >= 5 && card.data.chara_id == friend_chara_id {
                let mut friend = FriendState::new(Some(*id), index)?;
                friend.out_state = match self.friend_stage {
                    0 => FriendOutState::UnClicked,
                    1 => FriendOutState::BeforeUnlock,
                    2 => FriendOutState::AfterUnlock,
                    _ => FriendOutState::Away,
                };
                for slot in friend
                    .out_used
                    .iter_mut()
                    .take(self.friend_outgoing_used.max(0) as usize)
                {
                    *slot = true;
                }
                return Ok(friend);
            }
        }
        Ok(FriendState::default())
    }

    /// 帧 `baseGame` 段 → 局面底座（通用字段）
    ///
    /// 卡面羁绊以帧的人头为准（人头下标与卡组下标平行，帧缺人头时保留卡面默认）；
    /// 阶段归剧本侧（本层不设占位值）。
    ///
    /// 事件帧的选项按成功 / 失败两支挂进待处理队列（人头归属由剧本侧补齐）；
    /// **恢复失败的事件只跳过它自己**（记入 [`FrameOutcome::notes`]），帧其余部分照常生效
    /// ——事件数值缺失不该让整帧不可用，但也不能静默：失败原因交回调用方处理。
    pub fn to_base_game(&self, params: &BaseParams) -> Result<FrameOutcome> {
        let inherit = InheritInfo {
            blue_count: self.zhongma_blue_count,
            extra_count: params.extra_count,
        };
        let limit_base = Data::get()?.consts.five_status_limit_base;
        let mut game = BaseGame::new(self.uma_id, &self.card_id, inherit, limit_base, params.race_grades)?;

        self.apply_to_uma(&mut game.uma);
        for (i, person) in self.persons.iter().enumerate() {
            if let Some(card) = game.deck.get_mut(i) {
                card.friendship = person.friendship;
            }
        }
        game.turn = self.turn;
        game.friend = self.to_friend(params.friend_chara_id)?;
        game.train_level_count = self.train_level_count;
        game.distribution = self.person_distribution.clone();
        self.apply_key_events(&mut game, params.friend_first_event);

        let mut notes = vec![];
        if let Some(story) = &self.story {
            match story.to_event_data() {
                Ok(event) => game.unresolved_events.push(event),
                Err(err) => notes.push(FrameNote::EventSkipped {
                    id: story.id,
                    detail: format!("{err:#}"),
                }),
            }
        }
        Ok(FrameOutcome { game, notes })
    }

    /// 用 `keyEvents` 还原事件历史与友人阶段（不覆盖其它字段）
    ///
    /// 事件历史在协议里没有直接字段，只能由 `keyEvents`（真实事件 ID）换算，否则从真机
    /// 恢复出的局面会缺历史，rollout 的后续事件抽取与真机不一致。分族入账：
    ///
    /// - **友人首次点击**：命中本剧本 `friend_first_event` 且尚未出行 → 纠正为「已点击未出行」
    ///   （协议无法区分「未点击 / 已点击未出行」）。
    /// - **支援卡连续事件**：按**归属与序号**归到模拟表的合成模板 `8001`/`8002`/`8003`
    ///   （值为发生了该段的卡数）。注意**不能按末位判段**——角色/友人事件的序号是三位数
    ///   （如 `809001101` 是角色 9001 的第 101 号事件），按末位会被误算成第 1 段卡事件。
    /// - **其余族**（角色 / 友人事件等）：真实 ID 直接入账——模拟表若收录同族事件，ID
    ///   天然相同（实测 `friend_events` 同法编码），能对上即精确对齐；对不上也只是多一条
    ///   无人读的计数。
    pub fn apply_key_events(&self, game: &mut BaseGame, friend_first_event: i32) {
        if friend_first_event != 0
            && game.friend.card_state != FriendCardState::Empty
            && game.friend.out_state != FriendOutState::AfterUnlock
            && self.key_events.contains(&friend_first_event)
        {
            game.friend.out_state = FriendOutState::BeforeUnlock;
        }
        for &id in &self.key_events {
            if id == friend_first_event {
                continue;
            }
            match decode_event_id(id) {
                EventRef {
                    owner: EventOwner::SupportCard(_),
                    seq,
                } if (1..=3).contains(&seq) => {
                    *game.events.entry(8000 + seq).or_insert(0) += 1;
                }
                _ => {
                    *game.events.entry(id.max(0) as u32).or_insert(0) += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小合法帧体（必填字段齐全；可选字段一律不给）
    fn minimal() -> String {
        r#"{"scenarioId":14,"umaId":101901,"turn":0,"vital":100,"maxVital":100,"motivation":3,
            "fiveStatus":[1,2,3,4,5],"fiveStatusLimit":[1200,1200,1200,1200,1200],"skillPt":10,
            "skillScore":0,"totalHints":21,"trainLevelCount":[0,0,0,0,0],"failureRateBias":0,
            "isIll":false,"isQieZhe":false,"isAiJiao":false,"zhongMaBlueCount":[1,0,0,0,0],
            "isRacing":false,"cardId":[302424],"lockedTrainingId":-1,"friendship_noncard_yayoi":0,
            "friendship_noncard_reporter":0,"friend_stage":0,"friend_outgoingUsed":0,"playing_state":1}"#
            .to_string()
    }

    /// 层1：可选字段缺失落默认值，未知键被忽略
    #[test]
    fn test_optional_and_unknown() -> Result<()> {
        let raw = format!(
            r#"{{"baseGame":{}}}"#,
            minimal().replace("\"turn\":0", "\"turn\":0,\"unknownKey\":123")
        );
        let status = parse_base_status(&raw)?;
        assert_eq!(status.turn, 0);
        assert!(!status.is_xiao_qie, "缺失的可选字段落默认");
        assert_eq!(status.positive_thinking_count, 0);
        assert_eq!(status.source, None);
        assert_eq!(status.story, None);
        Ok(())
    }

    /// 层1：必填字段缺失必须报错
    #[test]
    fn test_required_missing() {
        let raw = format!(
            r#"{{"baseGame":{}}}"#,
            minimal().replace("\"zhongMaBlueCount\":[1,0,0,0,0],", "")
        );
        assert!(parse_base_status(&raw).is_err(), "必填字段缺失应报错");
    }

    /// 层1：缺 baseGame 外壳必须报错
    #[test]
    fn test_missing_envelope() {
        assert!(parse_base_status(r#"{"ramen":{}}"#).is_err());
    }

    /// 层1：失败率符号与生病位映射
    #[test]
    fn test_flags_mapping() -> Result<()> {
        let raw = format!(
            r#"{{"baseGame":{}}}"#,
            minimal()
                .replace("\"failureRateBias\":0", "\"failureRateBias\":-2")
                .replace("\"isIll\":false", "\"isIll\":true")
        );
        let flags = parse_base_status(&raw)?.to_flags();
        assert!(flags.good_trainer && !flags.bad_trainer, "负值 = 擅长训练");
        assert_eq!(flags.ill, Ill::UNKNOWN, "协议只给布尔 → 种类未知");

        let raw = format!(
            r#"{{"baseGame":{}}}"#,
            minimal().replace("\"failureRateBias\":0", "\"failureRateBias\":2")
        );
        let flags = parse_base_status(&raw)?.to_flags();
        assert!(flags.bad_trainer && !flags.good_trainer, "正值 = 不擅长训练");
        assert_eq!(flags.ill, Ill::NONE);
        Ok(())
    }

    /// 层1：局号缺失退化为马娘 ID
    #[test]
    fn test_epoch_key() -> Result<()> {
        let status = parse_base_status(&format!(r#"{{"baseGame":{}}}"#, minimal()))?;
        assert_eq!(status.epoch_key(), 101901);
        let with_id = format!(
            r#"{{"baseGame":{}}}"#,
            minimal().replace("\"turn\":0", "\"turn\":0,\"single_mode_chara_id\":6264")
        );
        assert_eq!(parse_base_status(&with_id)?.epoch_key(), 6264);
        Ok(())
    }

    /// 层1：事件 ID 分族解码（真机样本）
    #[test]
    fn test_decode_event_id() {
        // 卡事件：830 | 30242 | 002
        assert_eq!(
            decode_event_id(830242002),
            EventRef {
                owner: EventOwner::SupportCard(30242),
                seq: 2
            }
        );
        assert_eq!(
            decode_event_id(830137001),
            EventRef {
                owner: EventOwner::SupportCard(30137),
                seq: 1
            }
        );
        // 角色 / 友人事件：809 | 09001 | 101（序号三位数，按末位判段会误算）
        assert_eq!(
            decode_event_id(809001101),
            EventRef {
                owner: EventOwner::Character(9001),
                seq: 101
            }
        );
        // 未知形态
        assert_eq!(
            decode_event_id(12345),
            EventRef {
                owner: EventOwner::Unknown(12345),
                seq: 0
            }
        );
    }

    /// 层1：关键事件分族入账（卡事件归段、角色事件按真实 ID）
    #[test]
    fn test_apply_key_events_families() -> Result<()> {
        let raw = format!(
            r#"{{"baseGame":{}}}"#,
            minimal().replace("\"turn\":0", "\"turn\":0,\"keyEvents\":[830242001,830242002,809001101]")
        );
        let status = parse_base_status(&raw)?;
        let mut game = BaseGame::default();
        status.apply_key_events(&mut game, 0);
        assert_eq!(game.events.get(&8001).copied(), Some(1), "卡事件第 1 段");
        assert_eq!(game.events.get(&8002).copied(), Some(1), "卡事件第 2 段");
        assert_eq!(game.events.get(&8003).copied(), None, "未发生的段不入账");
        assert_eq!(
            game.events.get(&809001101).copied(),
            Some(1),
            "角色事件按真实 ID 入账（不得按末位误判成卡事件）"
        );
        Ok(())
    }

    /// 层1：技能点回加已花（协议发的是**剩余**值），其余字段以帧为准
    #[test]
    fn test_apply_to_uma() -> Result<()> {
        let raw = format!(
            r#"{{"baseGame":{}}}"#,
            minimal().replace("\"skillPt\":10", "\"skillPt\":10,\"skillPtSpent\":30")
        );
        let status = parse_base_status(&raw)?;
        let mut uma = Uma::default();
        status.apply_to_uma(&mut uma);
        assert_eq!(uma.skill_pt, 40, "剩余 10 + 已花 30");
        assert_eq!(uma.five_status, [1, 2, 3, 4, 5]);
        assert_eq!(uma.five_status_limit, [1200; 5]);
        assert_eq!(uma.vital, 100);
        assert_eq!(uma.total_hints, 21);
        Ok(())
    }
}
