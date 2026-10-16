//! L2 模块集成：`Runner` 驱动基础局一局到底 + 事件契约 + 决策上下文接缝
//!
//! 检查点：跑满 78 回合、阶段事件成对、决策按接缝取走、无错误事件、同配置可复现。

mod common;

use std::cell::RefCell;

use anyhow::Result;
use umaai::{
    Candidate, Config, DecisionRecord, DecisionSource, ErrorEvent, Frontend, GameStart, RunStart, Runner, StageCtx,
    Summary,
};
use umaai_core::Data;
use umaai_core::gamedata::EventChoice;
use umaai_core::testing::data_root;
use umasim::game::Trainer;
use umasim::game::base::BaseAction;
use umasim::game::base::basic::{BasicAction, BasicGame};

use crate::common::Checks;

/// Race 优先、其余取第 0 个动作，事件恒选第 0 组选项
///
/// 必须优先比赛：自选比赛未在期限内完成时 `Game::next` 会提前终止，跑不满 78 回合
/// （与跨 crate 端到端夹具同口径）。
struct RaceFirstTrainer;

impl Trainer<BasicGame> for RaceFirstTrainer {
    fn select_action(
        &self,
        _game: &BasicGame,
        actions: &[BasicAction],
        _rng: &mut rand::rngs::StdRng,
    ) -> Result<usize> {
        Ok(actions.iter().position(|a| a.0 == BaseAction::Race).unwrap_or(0))
    }

    fn select_choice(
        &self,
        _game: &BasicGame,
        _choices: &[Vec<EventChoice>],
        _rng: &mut rand::rngs::StdRng,
    ) -> Result<usize> {
        Ok(0)
    }
}

/// 记录事件的测试前端（引擎不打印，故事件只能由前端采集）
#[derive(Default)]
struct Recorder {
    kinds: Vec<String>,
    begins: usize,
    ends: usize,
    decisions: Vec<DecisionRecord>,
    errors: Vec<ErrorEvent>,
    summary: Option<Summary>,
    run_start: Option<RunStart>,
    game_start: Option<GameStart>,
}

impl Frontend for Recorder {
    fn on_run_start(&mut self, ctx: &RunStart) {
        self.kinds.push("run_start".to_string());
        self.run_start = Some(ctx.clone());
    }

    fn on_game_start(&mut self, ctx: &GameStart) {
        self.kinds.push("game_start".to_string());
        self.game_start = Some(*ctx);
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

    fn on_error(&mut self, err: &ErrorEvent) {
        self.kinds.push("error".to_string());
        self.errors.push(err.clone());
    }

    fn on_run_end(&mut self, summary: &Summary) {
        self.kinds.push("run_end".to_string());
        self.summary = Some(summary.clone());
    }
}

/// 桩决策源：预置若干决策，按「取走即清空」交付
struct StubSource {
    pending: RefCell<Vec<DecisionRecord>>,
}

impl StubSource {
    fn new(records: Vec<DecisionRecord>) -> Self {
        Self { pending: RefCell::new(records) }
    }
}

impl DecisionSource for StubSource {
    fn take_decision(&self) -> Option<DecisionRecord> {
        self.pending.borrow_mut().pop()
    }
}

/// 用默认配置构造基础局
fn newgame() -> Result<BasicGame> {
    let cfg = Config::load_from_data_root(data_root()?, None)?;
    BasicGame::newgame(cfg.sim.uma, &cfg.sim.cards, cfg.inherit(), cfg.race_grades()?)
}

/// 一局到底：终局、事件成对、决策接缝、无错误
#[test]
fn runner_full_game() -> Result<()> {
    Data::init(data_root()?)?;
    let mut c = Checks::new("Runner 基础局端到端");
    let cfg = Config::load_from_data_root(data_root()?, None)?;

    let sample = DecisionRecord {
        kind: "train".to_string(),
        action_index: 1,
        action_desc: "训练：耐力".to_string(),
        candidates: vec![
            Candidate {
                description: "训练：速度".to_string(),
                score: Some(100.0),
                n: 8,
            },
            Candidate {
                description: "训练：耐力".to_string(),
                score: Some(120.0),
                n: 8,
            },
        ],
        luck: None,
        extra: None,
    };
    let src = StubSource::new(vec![sample.clone()]);

    let mut fe = Recorder::default();
    let summary = {
        let mut runner = Runner::new(&cfg, &mut fe)?;
        runner.run_game(newgame()?, &RaceFirstTrainer, Some(&src))?
    };

    c.check(summary.turn == 77, "跑满 78 回合（0 基末回合 77）");
    c.check(summary.score > 0, "终局评分为正");
    c.check(summary.pt > 0, "终局 PT 为正");
    c.check(summary.five_status.iter().all(|v| *v >= 0), "五维非负");
    c.check(!summary.rank.is_empty(), "评分档次非空");
    c.check(fe.begins == fe.ends, "阶段事件成对");
    c.check(fe.begins == summary.stages, "阶段计数与事件一致");
    c.check(fe.kinds.first().is_some_and(|k| k == "run_start"), "首事件为 run_start");
    c.check(fe.kinds.last().is_some_and(|k| k == "run_end"), "末事件为 run_end");
    c.check(fe.errors.is_empty(), "无错误事件");
    c.check(fe.summary.as_ref() == Some(&summary), "run_end 携带终局汇总");
    c.check(
        fe.run_start.as_ref().is_some_and(|r| r.seed == 42),
        "run_start 携带种子",
    );
    c.check(
        fe.game_start.is_some_and(|g| g.max_turn == 77),
        "game_start 携带最大回合",
    );
    c.check(fe.decisions == vec![sample], "决策按接缝取走且内容一致");
    c.check(source_is_drained(&src), "接缝取走后清空");
    println!(
        "终局: score={} PT={} rank={} five={:?} stages={} decisions={}",
        summary.score, summary.pt, summary.rank, summary.five_status, summary.stages, summary.decisions
    );
    c.finish()
}

/// 不提供决策源时照常跑完，只是没有决策事件
#[test]
fn runner_without_source() -> Result<()> {
    Data::init(data_root()?)?;
    let mut c = Checks::new("Runner 无决策源");
    let cfg = Config::load_from_data_root(data_root()?, None)?;
    let mut fe = Recorder::default();
    let summary = {
        let mut runner = Runner::new(&cfg, &mut fe)?;
        runner.run_game(newgame()?, &RaceFirstTrainer, None)?
    };
    c.check(fe.decisions.is_empty(), "无决策事件");
    c.check(summary.decisions == 0, "决策计数为 0");
    c.check(summary.turn == 77, "仍跑满回合");
    c.finish()
}

/// 同配置重复运行逐位一致（种子来自配置）
#[test]
fn runner_reproducible() -> Result<()> {
    Data::init(data_root()?)?;
    let mut c = Checks::new("Runner 可复现");
    let cfg = Config::load_from_data_root(data_root()?, None)?;
    let mut a = Recorder::default();
    let mut b = Recorder::default();
    let first = {
        let mut runner = Runner::new(&cfg, &mut a)?;
        runner.run_game(newgame()?, &RaceFirstTrainer, None)?
    };
    let second = {
        let mut runner = Runner::new(&cfg, &mut b)?;
        runner.run_game(newgame()?, &RaceFirstTrainer, None)?
    };
    c.check(first == second, "两次运行终局逐项一致");
    c.check(a.kinds == b.kinds, "两次运行事件序列一致");
    println!("复现: score={} PT={} stages={}", first.score, first.pt, first.stages);
    c.finish()
}

/// 桩源是否已被取空（只能经内部状态观察）
fn source_is_drained(src: &StubSource) -> bool {
    src.pending.borrow().is_empty()
}
