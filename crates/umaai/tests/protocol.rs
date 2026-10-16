//! L2 模块集成：协议通用段解析（真实帧夹具 + 终局帧三道闸）
//!
//! 夹具取自真机快照，**只保留 `baseGame` 段**（剧本段不进本仓）：回合 0 / 13 / 31 三帧
//! 覆盖「开局 / 事件帧 + 关键事件 / 中期」三态；终局帧原样保留，且其局号与回合帧**不同**
//! ——正好验证「固定文件名会残留上一局、必须校验局号」。

mod common;

use anyhow::Result;
use umaai::{BaseParams, Config, FinalScoreFrame, parse_base_status};
use umaai_core::Data;
use umaai_core::testing::data_root;
use umasim::game::{FriendCardState, FriendOutState};

use crate::common::Checks;

/// 帧夹具（真机快照，仅保留 baseGame 段）
const TURN0: &str = include_str!("fixtures/turn0.json");
/// 事件帧夹具（带 `story` 与 4 条关键事件）
const TURN13: &str = include_str!("fixtures/turn13.json");
/// 中期帧夹具（已出行 1 段、3 场生涯比赛）
const TURN31: &str = include_str!("fixtures/turn31.json");
/// 终局帧夹具（与上面三帧**不是同一局**）
const FINAL: &str = include_str!("fixtures/final.json");

/// 夹具里那张剧本友人卡的**角色 ID**（由宿主按剧本档案注入，引擎不含此常量）
const FRIEND_CHARA_ID: u32 = 9001;

/// 载入配置与真实数据
fn setup() -> Result<Config> {
    Data::init(data_root()?)?;
    Config::load_from_data_root(data_root()?, None)
}

/// 造转换参数（剧本相关的量由宿主注入）
fn params(cfg: &Config) -> Result<BaseParams<'_>> {
    Ok(BaseParams {
        extra_count: cfg.inherit().extra_count,
        race_grades: cfg.race_grades()?,
        friend_chara_id: FRIEND_CHARA_ID,
        friend_first_event: 0,
    })
}

/// 开局帧：真实字段逐项落到局面底座
#[test]
fn protocol_turn0() -> Result<()> {
    let mut c = Checks::new("协议开局帧");
    let cfg = setup()?;
    let status = parse_base_status(TURN0)?;

    c.check(status.scenario_id == 14, "剧本 ID 原样保留（引擎不解释）");
    c.check(status.turn == 0 && status.uma_id == 109701, "回合与马娘取自帧");
    c.check(status.epoch_key() == 6264, "局号取自帧");
    // 帧里有旧实现未建模的字段（cardRecord / saihou / islegal 等），解析必须照样成功
    c.check(status.persons.len() == 6, "六个人头（含未建模的 cardRecord 字段）");
    c.check(status.source.as_deref() == Some("command"), "阶段来源原样保留");

    let out = status.to_base_game(&params(&cfg)?)?;
    let game = out.game;
    c.check(out.notes.is_empty(), "真实帧没有需要跳过的事件");
    c.check(game.turn == 0, "回合落到局面");
    c.check(game.uma.five_status == [186, 132, 150, 132, 146], "五维以帧为准");
    c.check(
        game.uma.five_status_limit == [3196, 2432, 2200, 2200, 2464],
        "五维上限以帧为准",
    );
    c.check(game.uma.skill_pt == 293, "技能点 = 剩余 + 已花（本帧未花）");
    c.check(game.uma.vital == 100 && game.uma.motivation == 3, "体力与干劲以帧为准");
    c.check(game.uma.flags.xiaoqie && !game.uma.flags.qiezhe, "切者 / 小切标志");
    c.check(game.uma.flags.ill.is_empty(), "本帧无病");
    c.check(
        game.deck[0].friendship == status.persons[0].friendship,
        "卡面羁绊按人头回填",
    );
    c.check(game.distribution == status.person_distribution, "人头分布原样落到局面");
    c.check(game.friend.card_state != FriendCardState::Empty, "识别出剧本友人卡");
    c.check(
        game.friend.out_state == FriendOutState::UnClicked,
        "友人阶段 0 = 未点击",
    );
    c.check(game.events.is_empty(), "本帧无关键事件");
    c.check(game.unresolved_events.is_empty(), "本帧无事件段");
    println!(
        "开局: five={:?} pt={} 友人={:?} 分布={:?}",
        game.uma.five_status, game.uma.skill_pt, game.friend.card_state, game.distribution
    );
    c.finish()
}

/// 事件帧：关键事件还原事件历史，事件段可解析
#[test]
fn protocol_turn13() -> Result<()> {
    let mut c = Checks::new("协议事件帧");
    let cfg = setup()?;
    let status = parse_base_status(TURN13)?;
    c.check(status.turn == 13, "回合取自帧");
    c.check(status.key_events.len() == 4, "四条关键事件");
    let story = status.story.as_ref().expect("本帧带事件段");
    c.check(story.id == 809001104, "事件 ID 解析");
    c.check(!story.choices.is_empty(), "事件选项非空");
    let value = story.choices[0][0].success_effect_value.as_ref().expect("带成功数值");
    c.check(value.status_pt().is_ok(), "事件数值槽位可解析");

    let out = status.to_base_game(&params(&cfg)?)?;
    let game = out.game;
    c.check(out.notes.is_empty(), "真实帧没有需要跳过的事件");
    // 四条关键事件里三条是卡事件第 1 段（830137001 / 830297001 / 830304001），
    // 一条是角色 9001 的友人事件（809001101，序号 101）——按末位判段会把它误算成卡事件
    c.check(
        game.events.get(&8001).copied() == Some(3),
        &format!("卡事件第 1 段计数: {:?}", game.events),
    );
    c.check(
        game.events.get(&809001101).copied() == Some(1),
        &format!("角色事件按真实 ID 入账: {:?}", game.events),
    );
    c.check(
        game.uma
            .free_races
            .iter()
            .any(|f| f.start_turn <= 13 && 13 <= f.end_turn),
        "生涯赛程按比赛等级表算好",
    );
    c.check(game.uma.win_races.contains_turn(11), "比赛历史 11 已标记");
    // 事件帧 → 待处理事件：本帧每个选项只带成功数值，故各为单支必定触发
    c.check(game.unresolved_events.len() == 1, "事件帧挂出 1 个待处理事件");
    let event = &game.unresolved_events[0];
    c.check(event.id == 809001104, "待处理事件 ID 与帧一致");
    c.check(event.choices.len() == story.choices.len(), "选项组数与帧一致");
    c.check(event.player_select, "多选项事件交给训练员决策");
    c.check(
        event.choices.iter().all(|g| g.len() == 1 && g[0].prob == 100),
        "帧内选项只有成功数值 → 单支必定触发",
    );
    c.check(
        event.choices[0][0].value.vital == 40 && event.choices[0][0].value.motivation == 1,
        "事件数值按槽位落到效果（体力 40 / 干劲 1）",
    );
    println!(
        "事件帧: events={:?} 事件 #{} 待处理={}",
        game.events,
        story.id,
        game.unresolved_events.len()
    );
    c.finish()
}

/// 中期帧：友人已出行、多场生涯比赛、事件帧挂出待处理事件
#[test]
fn protocol_turn31() -> Result<()> {
    let mut c = Checks::new("协议中期帧");
    let cfg = setup()?;
    let status = parse_base_status(TURN31)?;
    let out = status.to_base_game(&params(&cfg)?)?;
    let game = out.game;
    c.check(out.notes.is_empty(), "真实帧没有需要跳过的事件");

    c.check(game.uma.skill_pt == 2100, "技能点取自帧");
    c.check(
        game.friend.out_state == FriendOutState::AfterUnlock,
        "友人阶段 2 = 已出行",
    );
    c.check(game.friend.out_used[0] && !game.friend.out_used[1], "出行段标记前 1 段");
    for turn in [11, 23, 30] {
        c.check(
            game.uma.win_races.contains_turn(turn),
            &format!("比赛历史 {turn} 已标记"),
        );
    }
    c.check(game.train_level_count == [12, 11, 5, 4, 13], "设施等级计数取自帧");
    c.check(game.deck[0].friendship == 100, "羁绊封顶 100 取自帧");
    c.check(
        game.unresolved_events.first().map(|e| e.id) == Some(801041001),
        "本帧事件也挂成待处理事件",
    );
    println!("中期: 出行={:?} 赛程={:?}", game.friend.out_state, game.uma.win_races);
    c.finish()
}

/// 终局帧：解析 + 三道闸
#[test]
fn protocol_final_frame() -> Result<()> {
    let mut c = Checks::new("终局帧闸门");
    Data::init(data_root()?)?;
    let frame = FinalScoreFrame::parse(FINAL)?;
    c.check(frame.scenario_id == 14, "剧本 ID 原样保留");
    c.check(frame.is_usable(), "真实终局帧可用");
    c.check(frame.calc_score()? > 0, "终局评分可算");
    c.check(!frame.rank_name()?.is_empty(), "档次名非空");
    // 真机实测：固定文件名会残留上一局的终局帧（本夹具局号 6266，回合帧 6264）
    c.check(frame.epoch_key() == Some(6266), "局号取自帧");
    c.check(frame.matches_epoch(6266), "局号匹配当前局");
    c.check(!frame.matches_epoch(6264), "残留的上一局终局帧必须被拒");

    // 坏帧：字段在场但全零
    let zero = FINAL.replace("\"fiveStatus\": [", "\"fiveStatus\": [0, 0, 0, 0, 0], \"ignored\": [");
    let bad = FinalScoreFrame::parse(&zero)?;
    c.check(!bad.is_usable(), "全零坏帧判不可用");
    println!("终局: score={} rank={}", frame.calc_score()?, frame.rank_name()?);
    c.finish()
}

/// 坏帧路径：缺外壳 / 缺必填字段
#[test]
fn protocol_bad_frames() -> Result<()> {
    let mut c = Checks::new("协议坏帧路径");
    c.check(parse_base_status("{}").is_err(), "缺 baseGame 外壳报错");
    c.check(parse_base_status(r#"{"baseGame":{}}"#).is_err(), "空 baseGame 报错");
    c.check(
        parse_base_status(&TURN0.replace("\"umaId\"", "\"umaIdRenamed\"")).is_err(),
        "必填字段改名（插件漂移）必须报错而不是静默按 0",
    );
    c.finish()
}

/// 事件数值缺失：只跳过该事件，帧其余部分照常生效（原因进 notes，不静默）
#[test]
fn protocol_event_skipped() -> Result<()> {
    let mut c = Checks::new("事件恢复失败降级");
    let cfg = setup()?;
    // 真机开局帧 + 一个没有数值的事件（选项缺 SuccessEffectValue / FailedEffectValue）
    let mut frame: serde_json::Value = serde_json::from_str(TURN0)?;
    frame["baseGame"]["story"] = serde_json::json!({
        "Id": 9001,
        "Name": "坏事件",
        "Choices": [[{ "Option": "A" }]]
    });
    let status = parse_base_status(&frame.to_string())?;
    let out = status.to_base_game(&params(&cfg)?)?;

    c.check(out.game.unresolved_events.is_empty(), "数值缺失的事件不进队列");
    c.check(out.notes.len() == 1, "跳过原因被记录成 note");
    match out.notes.first() {
        Some(umaai::FrameNote::EventSkipped { id, detail }) => {
            c.check(*id == 9001, "note 指明事件 ID");
            c.check(!detail.is_empty(), "note 带原因");
        }
        None => c.check(false, "note 应为 EventSkipped"),
    }
    // 帧其余部分照常生效
    c.check(
        out.game.turn == 0 && out.game.uma.five_status == [186, 132, 150, 132, 146],
        "帧其余状态不受影响",
    );
    println!("跳过事件: {:?}", out.notes);
    c.finish()
}
