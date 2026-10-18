//! L3 端到端：会话把合成的基础局帧序列驱动成「解析 → 决策 → 记录 → 终局裁决」
//!
//! 真机拉面帧驱动不了 `BasicGame`（剧本段与 persons 构造归剧本仓），故帧序列在测试侧由
//! 真实基础局面合成：训练帧取决策点局面，事件帧分两选项（走决策）与单选项（不决策）两支，
//! 终局帧覆盖三道闸（契约必填由解析保证 / 全零判废 / 局号匹配）。另有一例 `#[ignore]`
//! 走真通道（`FileChannel` → `Session::run`）。

use std::cell::RefCell;
use std::collections::VecDeque;
#[cfg(feature = "file-channel")]
use std::fs;
#[cfg(feature = "file-channel")]
use std::time::Duration;

use anyhow::{Result, bail};
use rand::SeedableRng;
use rand::rngs::StdRng;
#[cfg(feature = "file-channel")]
use umaai::FileChannel;
#[cfg(feature = "file-channel")]
use umaai::RunStop;
#[cfg(feature = "file-channel")]
use umaai::protocol::channel::{FRAME_FINAL_SCORE, FRAME_THIS_TURN, plugin_dir};
use umaai::protocol::story::{StoryChoice, StoryEffectValue};
use umaai::{
    BaseParams, BaseStatus, Candidate, Config, DecisionRecord, DecisionSource, FinalScoreFrame, FrameKind, FrameRoute,
    Frontend, GameStart, RawFrame, RunStart, Session, StageCtx, StoryStatus, Summary,
};
use umaai_core::Data;
use umaai_core::testing::data_root;
use umaai_tests::Checker;
use umaai_tests::testing::{RaceFirstTrainer, basic_newgame};
use umasim::game::Game;
use umasim::game::base::basic::BasicGame;
use umasim::game::base::stage::BasicStage;

/// 合成帧用的局号（真机由插件给出；终局帧必须与它一致才会被采纳）
const EPOCH: u64 = 6264;

/// 预置决策的记录器（`pop_front`：按放入顺序交付，取走即清空）
struct StubSource {
    pending: RefCell<VecDeque<DecisionRecord>>,
}

impl StubSource {
    /// 按顺序预置若干决策
    fn new(records: Vec<DecisionRecord>) -> Self {
        Self {
            pending: RefCell::new(records.into()),
        }
    }

    /// 剩余未取走的决策数（只能经内部状态观察）
    fn remaining(&self) -> usize {
        self.pending.borrow().len()
    }
}

impl DecisionSource for StubSource {
    fn take_decision(&self) -> Option<DecisionRecord> {
        self.pending.borrow_mut().pop_front()
    }
}

/// 采集前端事件（引擎不打印，事件只能由前端观察）
#[derive(Default)]
struct Recorder {
    kinds: Vec<String>,
    begins: usize,
    ends: usize,
    decisions: Vec<DecisionRecord>,
    run_start: Option<RunStart>,
    game_starts: Vec<GameStart>,
    run_end: Option<Summary>,
}

impl Frontend for Recorder {
    fn on_run_start(&mut self, ctx: &RunStart) {
        self.kinds.push("run_start".to_string());
        self.run_start = Some(ctx.clone());
    }

    fn on_game_start(&mut self, ctx: &GameStart) {
        self.kinds.push("game_start".to_string());
        self.game_starts.push(*ctx);
    }

    fn on_stage_begin(&mut self, _ctx: &StageCtx) {
        self.kinds.push("begin".to_string());
        self.begins += 1;
    }

    fn on_stage_end(&mut self) {
        self.kinds.push("end".to_string());
        self.ends += 1;
    }

    fn on_decision(&mut self, rec: &DecisionRecord) {
        self.kinds.push(format!("decision:{}", rec.kind));
        self.decisions.push(rec.clone());
    }

    fn on_run_end(&mut self, summary: &Summary) {
        self.kinds.push("run_end".to_string());
        self.run_end = Some(summary.clone());
    }
}

/// 把基础局推进到首回合训练决策点（与搜索的生产调用点同位置）
fn decision_point() -> Result<BasicGame> {
    let mut game = basic_newgame()?; // 夹具已注入规则主种子（与配置默认层同值）
    let mut rng = StdRng::seed_from_u64(0);
    for _ in 0..16 {
        if game.stage == BasicStage::Train {
            return Ok(game);
        }
        game.run_stage(&RaceFirstTrainer, &mut rng)?;
        if !game.next() {
            bail!("首回合未抵达训练阶段");
        }
    }
    bail!("推进阶段次数超限，未抵达训练阶段")
}

/// 由局面合成一帧 `baseGame` 段（真机帧由插件产出，这里只补通用字段）
fn turn_frame(src: &BasicGame, turn: i32, source: &str, story: Option<StoryStatus>) -> Result<String> {
    let status = BaseStatus {
        scenario_id: 14,
        uma_id: src.uma.uma_id,
        turn,
        vital: src.uma.vital,
        max_vital: src.uma.max_vital,
        motivation: src.uma.motivation,
        five_status: src.uma.five_status,
        five_status_limit: src.uma.five_status_limit,
        skill_pt: src.uma.skill_pt,
        skill_score: src.uma.skill_score,
        total_hints: src.uma.total_hints,
        train_level_count: src.train_level_count,
        failure_rate_bias: 0,
        is_ill: false,
        is_qiezhe: false,
        is_aijiao: false,
        zhongma_blue_count: src.inherit.blue_count,
        is_racing: false,
        card_id: src.deck.iter().map(|c| c.card_id * 10 + c.rank).collect(),
        locked_training_id: -1,
        friendship_noncard_yayoi: 0,
        friendship_noncard_reporter: 0,
        friend_stage: 0,
        friend_outgoing_used: 0,
        playing_state: 1,
        person_distribution: src.distribution.clone(),
        story,
        source: Some(source.to_string()),
        single_mode_chara_id: Some(EPOCH),
        ..Default::default()
    };
    Ok(serde_json::json!({ "baseGame": status }).to_string())
}

/// 一个数值齐全的选项效果（10 槽：五维 + PT / Hint / 体力 / 羁绊 / 干劲）
fn effect(values: [i32; 10]) -> StoryEffectValue {
    StoryEffectValue {
        values: values.to_vec(),
        ..Default::default()
    }
}

/// 合成事件：`choices` 为选项组数（>1 → 帧事件交给训练员决策）
fn story(choices: usize) -> StoryStatus {
    let groups = (0..choices)
        .map(|i| {
            let mut values = [0; 10];
            values[i.min(4)] = 10;
            vec![StoryChoice {
                option: format!("选项{i}"),
                success_effect_value: Some(effect(values)),
                ..Default::default()
            }]
        })
        .collect();
    StoryStatus {
        id: 7001,
        name: "合成事件".to_string(),
        trigger_name: "トレーナー".to_string(),
        choices: groups,
    }
}

/// 合成终局帧（`five` 全零即坏帧）
fn final_frame(epoch: u64, five: [i32; 5]) -> Result<String> {
    let frame = FinalScoreFrame {
        scenario_id: 14,
        single_mode_chara_id: Some(epoch),
        uma_id: 101901,
        uma_star: 5,
        turn: 77,
        state: 3,
        playing_state: 5,
        five_status: five,
        five_status_limit: [1200; 5],
        skill_pt: 300,
        total_hints: 20,
        skill_pt_spent: 0,
        inherit_gains: vec![],
    };
    Ok(serde_json::to_string(&frame)?)
}

/// 训练决策（带评分 → 可挂运气分）
fn train_record() -> DecisionRecord {
    DecisionRecord {
        kind: "train".to_string(),
        action_index: 0,
        action_desc: "训练：速度".to_string(),
        candidates: vec![
            Candidate {
                description: "训练：速度".to_string(),
                score: Some(1000.0),
                n: 4,
            },
            Candidate {
                description: "休息".to_string(),
                score: Some(900.0),
                n: 4,
            },
        ],
        luck: None,
        extra: None,
    }
}

/// 帧 → 局面的构造（剧本侧职责：persons 构造与阶段判定；基础局用 `init_persons`）
///
/// 合成的帧都是决策点帧（command 走训练决策、event 走事件决策），故阶段直接落 `Train`；
/// 真实剧本在这里按帧内 `playing_state` / `source` 判定自己的阶段。
fn build_basic(base: umasim::game::base::BaseGame) -> Result<BasicGame> {
    let mut game = BasicGame {
        base,
        persons: vec![],
        stage: BasicStage::Train,
        // 剧本自持的策略流：会话在重建局面后会注入主种子并派生各轴，这里留空即可
        strategy: None,
    };
    game.init_persons()?;
    Ok(game)
}

/// 合成帧序列端到端：训练帧决策 + 事件帧决策 + 终局帧三道闸
#[test]
fn session_frame_sequence_end_to_end() -> Result<()> {
    Data::init(data_root()?)?;
    let mut c = Checker::new("会话：合成帧序列端到端");
    let cfg = Config::load_from_data_root(data_root()?, None)?;

    let root_game = decision_point()?;
    let train = RawFrame {
        kind: FrameKind::ThisTurn,
        contents: turn_frame(&root_game, root_game.turn, "command", None)?,
    };
    let event_multi = RawFrame {
        kind: FrameKind::ThisTurn,
        contents: turn_frame(&root_game, 1, "event", Some(story(2)))?,
    };
    let event_single = RawFrame {
        kind: FrameKind::ThisTurn,
        contents: turn_frame(&root_game, 2, "event", Some(story(1)))?,
    };
    let final_stale = RawFrame {
        kind: FrameKind::FinalScore,
        contents: final_frame(EPOCH - 1, [800; 5])?,
    };
    let final_zero = RawFrame {
        kind: FrameKind::FinalScore,
        contents: final_frame(EPOCH, [0; 5])?,
    };
    let final_ok = RawFrame {
        kind: FrameKind::FinalScore,
        contents: final_frame(EPOCH, [800, 700, 600, 500, 900])?,
    };

    let trainer = RaceFirstTrainer;
    // 第三条为哨兵：单选项事件帧若误消费决策源就会被观察到最后剩 0
    let src = StubSource::new(vec![
        train_record(),
        DecisionRecord::action_only("event", "选项1"),
        DecisionRecord::action_only("event", "哨兵：不该被消费"),
    ]);
    let mut fe = Recorder::default();
    let params = BaseParams {
        extra_count: cfg.inherit().extra_count,
        race_grades: cfg.race_grades()?,
        friend_chara_id: 0,
        friend_first_event: 0,
    };

    // 会话借走前端与决策源，故先收结果，块外再核对前端事件
    let (o1, o2, o3, o4, o5, o6, after_multi, counts, epoch, left) = {
        let mut session = Session::new(&cfg, &trainer, &mut fe, params, Some(&src), build_basic)?;
        let o1 = session.step(&train)?;
        let o2 = session.step(&event_multi)?;
        let after_multi = src.remaining();
        let o3 = session.step(&event_single)?;
        let o4 = session.step(&final_stale)?;
        let o5 = session.step(&final_zero)?;
        let o6 = session.step(&final_ok)?;
        (
            o1,
            o2,
            o3,
            o4,
            o5,
            o6,
            after_multi,
            session.counts(),
            session.epoch_key(),
            src.remaining(),
        )
    };

    // 帧1：训练帧
    c.check(o1.route == FrameRoute::Training, "训练帧路由到训练决策");
    c.check(
        o1.decision.as_ref().map(|d| d.kind.as_str()) == Some("train"),
        "训练决策已记录且类别为 train",
    );
    c.check(
        o1.decision.as_ref().is_some_and(|d| d.luck.is_some()),
        "带评分决策挂上运气分快照",
    );
    c.check(o1.turn_luck.is_none(), "首个回合运气为空（没有上一回合基线）");
    c.check(epoch == Some(EPOCH), "局号取自帧");
    c.check(counts == (6, 2), "六帧、两条决策（训练 + 多选项事件）");

    // 帧2/3：事件帧路由
    c.check(o2.route == FrameRoute::Event, "source=event 帧路由到事件决策");
    c.check(o2.final_frame.is_none(), "回合帧不带终局结果");
    c.check(after_multi == 1, "多选项事件消费一条决策记录");
    c.check(o3.route == FrameRoute::Event, "单选项事件帧同样按事件路由");
    c.check(o3.decision.is_none(), "单选项事件不记决策（无选项可选）");
    c.check(left == 1, "单选项事件不消费决策源（哨兵仍在）");

    // 帧4/5/6：终局帧三道闸
    let stale = o4.final_frame.as_ref().expect("残留终局帧应给出裁决");
    c.check(
        !stale.accepted && stale.reject_reason.is_some(),
        "局号不匹配的残留终局帧被拒并给原因",
    );
    let zero = o5.final_frame.as_ref().expect("全零终局帧应给出裁决");
    c.check(!zero.accepted, "五维全零的坏帧判废");
    let ok = o6.final_frame.as_ref().expect("匹配终局帧应给出裁决");
    c.check(
        ok.accepted && ok.score.is_some_and(|s| s > 0),
        "匹配终局帧被采纳且评分为正",
    );
    c.check(ok.rank.as_ref().is_some_and(|r| !r.is_empty()), "档次名非空");

    // 前端事件
    c.check(fe.kinds.first().is_some_and(|k| k == "run_start"), "首事件为 run_start");
    c.check(fe.game_starts.len() == 1, "局号未变只发一次 game_start");
    c.check(
        fe.game_starts.first().is_some_and(|g| g.epoch_key == EPOCH),
        "game_start 携带局号",
    );
    c.check(fe.begins == 3 && fe.ends == 3, "三个回合帧各一对阶段事件");
    c.check(fe.decisions.len() == 2, "前端收到两条决策");
    c.check(fe.kinds.last().is_some_and(|k| k == "run_end"), "末事件为 run_end");
    c.check(
        fe.run_end.as_ref().is_some_and(|s| Some(s.score) == ok.score),
        "run_end 汇总分与终局帧一致",
    );
    println!(
        "终局: score={:?} rank={:?} frames={:?} decisions={}",
        ok.score, ok.rank, counts.0, counts.1
    );
    c.finish()
}

/// 真通道端到端：`FileChannel` → `Session::run`（依赖文件系统事件派发，默认不跑）
#[cfg(feature = "file-channel")]
#[test]
#[ignore = "走真 watcher（手动：cargo test -p umaai-tests --release --test session_basic -- --ignored --nocapture）"]
fn session_runs_on_real_channel() -> Result<()> {
    Data::init(data_root()?)?;
    let mut c = Checker::new("会话：真通道路径");
    let cfg = Config::load_from_data_root(data_root()?, None)?;

    let root = std::env::temp_dir().join(format!("umaai_session_it_{}", std::process::id()));
    let dir = plugin_dir(&root);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&dir)?;

    // 预置两份白名单文件：启动兜底各推一份
    let root_game = decision_point()?;
    fs::write(
        dir.join(FRAME_THIS_TURN),
        turn_frame(&root_game, root_game.turn, "command", None)?,
    )?;
    fs::write(dir.join(FRAME_FINAL_SCORE), final_frame(EPOCH, [800; 5])?)?;

    let trainer = RaceFirstTrainer;
    let src = StubSource::new(vec![train_record()]);
    let mut fe = Recorder::default();
    let params = BaseParams {
        extra_count: cfg.inherit().extra_count,
        race_grades: cfg.race_grades()?,
        friend_chara_id: 0,
        friend_first_event: 0,
    };

    let summary = {
        let mut ch = FileChannel::open(&root)?;
        let mut session = Session::new(&cfg, &trainer, &mut fe, params, Some(&src), build_basic)?;
        session.run(&mut ch, Duration::from_millis(500), Duration::from_secs(3))?
    };

    c.check(summary.stop == RunStop::FinalAccepted, "终局帧采纳即收局");
    c.check(summary.frames == 2, "启动兜底送达两帧");
    c.check(summary.decisions == 1, "训练帧记录一条决策");
    c.check(summary.final_frame.as_ref().is_some_and(|f| f.accepted), "终局帧被采纳");
    c.check(fe.run_end.is_some(), "run_end 已下发");

    let _ = fs::remove_dir_all(&root);
    c.finish()
}
