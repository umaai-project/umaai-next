//! 会话：把帧驱动的「解析 → 决策 → 记录」闭环串起来
//!
//! 一帧一决策、局面不跨帧：每帧由通用段重建局面（`to_base_game`），处理完即丢——真机
//! 下一帧就是事实源。帧路由按 `source` 分两支：`event` 帧走**事件决策**（帧内事件逐个
//! `Game::run_event`，多选项时由训练员决定），其余帧走**训练决策**（`Game::run_stage`）；
//! 终局帧走三道闸（契约必填 / 全零判废 / 局号匹配）后裁决。
//!
//! 剧本相关的东西全在宿主：帧 → 局面的构造（`build`，含 persons 构造与阶段覆写）、剧本
//! 参数（[`BaseParams`]）、训练员、决策源、前端。引擎不含剧本常量，也不打印、不落盘。

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rand::SeedableRng;
use rand::rngs::StdRng;
use umaai_core::Data;
use umaai_core::gamedata::EventData;
use umasim::game::base::BaseGame;
use umasim::game::{Game, Trainer};

use crate::config::Config;
use crate::decision::{DecisionRecord, DecisionSource};
use crate::frontend::{Frontend, GameStart, RunStart, StageCtx};
use crate::protocol::base::{BaseParams, FrameNote, FrameOutcome, parse_base_status};
use crate::protocol::channel::{ChannelNote, FileChannel, FrameKind, RawFrame};
use crate::protocol::final_score::FinalScoreFrame;
use crate::record::LuckScorer;
use crate::runner::Summary;

/// 帧 `source` 取该值时按事件帧路由，其余值一律走训练决策
const SOURCE_EVENT: &str = "event";

/// 帧路由（决定本帧走哪个决策入口）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRoute {
    /// 训练 / 指令帧：`Game::run_stage`
    Training,
    /// 事件帧（`source = "event"`）：`Game::run_event`
    Event,
    /// 终局帧：不走决策
    FinalScore,
}

/// 一帧的处理结果（引擎不打印；宿主决定呈现与落盘）
#[derive(Debug, Clone)]
pub struct StepOutcome {
    /// 本帧路由
    pub route: FrameRoute,
    /// 本帧产生的决策（无决策 = `None`）
    pub decision: Option<DecisionRecord>,
    /// 本帧决策的回合运气分（仅带评分决策挂载）
    pub turn_luck: Option<f64>,
    /// 终局帧裁决结果（仅终局帧）
    pub final_frame: Option<FinalFrameOutcome>,
    /// 帧解析中的非致命问题（事件数值缺失等）
    pub notes: Vec<FrameNote>,
}

/// 终局帧裁决结果（三道闸逐层给结论，不静默丢弃）
#[derive(Debug, Clone)]
pub struct FinalFrameOutcome {
    /// 终局帧原文
    pub frame: FinalScoreFrame,
    /// 是否被采纳
    pub accepted: bool,
    /// 未采纳原因（`accepted = false` 时必有）
    pub reject_reason: Option<String>,
    /// 终局评分（采纳后计算）
    pub score: Option<i32>,
    /// 评分档次名（采纳后计算）
    pub rank: Option<String>,
}

/// 会话停止原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStop {
    /// 终局帧已被采纳（本局结束）
    FinalAccepted,
    /// 连续 `idle_limit` 没有新帧（通道空转）
    Idle,
}

/// 会话汇总（帧驱动口径；终局帧未采纳时 `final_frame` 为 `None`）
#[derive(Debug, Clone)]
pub struct SessionSummary {
    /// 停止原因
    pub stop: RunStop,
    /// 已处理帧数
    pub frames: usize,
    /// 已记录决策数
    pub decisions: usize,
    /// 当前局号（未收到回合帧时为 `None`）
    pub epoch_key: Option<u64>,
    /// 被采纳的终局帧结果
    pub final_frame: Option<FinalFrameOutcome>,
    /// 通道自愈痕迹（取走即清空）
    pub notes: Vec<ChannelNote>,
}

/// 帧驱动会话：一帧一决策，局面不跨帧
pub struct Session<'a, G, T>
where
    G: Game,
    T: Trainer<G>,
{
    cfg: &'a Config,
    trainer: &'a T,
    fe: &'a mut dyn Frontend,
    src: Option<&'a dyn DecisionSource>,
    params: BaseParams<'a>,
    build: Box<dyn FnMut(BaseGame) -> Result<G> + 'a>,
    rng: StdRng,
    scorer: LuckScorer,
    epoch: Option<u64>,
    frames: usize,
    decisions: usize,
    final_frame: Option<FinalFrameOutcome>,
}

impl<'a, G: Game, T: Trainer<G>> Session<'a, G, T> {
    /// 建立会话；要求全局静态数据已初始化（宿主先调 `Data::init`）
    ///
    /// `build` 由剧本侧提供：把帧通用段重建出的局面底座接成该剧本的局面（含 persons
    /// 构造与阶段覆写）。`src` 为可选决策上下文提供方，不提供则只计数、记录里没有候选。
    pub fn new(
        cfg: &'a Config,
        trainer: &'a T,
        fe: &'a mut dyn Frontend,
        params: BaseParams<'a>,
        src: Option<&'a dyn DecisionSource>,
        build: impl FnMut(BaseGame) -> Result<G> + 'a,
    ) -> Result<Self> {
        Data::try_get().context("全局静态数据未初始化：宿主应先调用 umaai_core::Data::init(data_root)")?;
        Ok(Self {
            cfg,
            trainer,
            fe,
            src,
            params,
            build: Box::new(build),
            rng: StdRng::seed_from_u64(cfg.sim.seed),
            scorer: LuckScorer::new(cfg.record.luck_turn_bonus),
            epoch: None,
            frames: 0,
            decisions: 0,
            final_frame: None,
        })
    }

    /// 处理一帧：帧进、结果出（不打印、不落盘）
    pub fn step(&mut self, frame: &RawFrame) -> Result<StepOutcome> {
        self.frames += 1;
        match frame.kind {
            FrameKind::ThisTurn => self.step_turn(&frame.contents),
            FrameKind::FinalScore => self.step_final(&frame.contents),
        }
    }

    /// 驱动通道：终局帧被采纳或连续 `idle_limit` 无帧即返回（不会无限阻塞）
    ///
    /// CLI 的顶层循环按「一局」调用一次；通道自愈痕迹经 [`SessionSummary::notes`] 交回。
    /// 单帧处理失败原样返回 `Err`（宿主决定是否继续与如何呈现）；终局帧被拒不算失败，
    /// 会继续等匹配的终局帧。
    pub fn run(&mut self, ch: &mut FileChannel, timeout: Duration, idle_limit: Duration) -> Result<SessionSummary> {
        let mut notes = Vec::new();
        let mut idle_since = Instant::now();
        loop {
            match ch.next_frame(timeout)? {
                Some(frame) => {
                    idle_since = Instant::now();
                    self.step(&frame)?;
                    if self.final_frame.is_some() {
                        notes.append(&mut ch.drain_notes());
                        return Ok(self.summary(RunStop::FinalAccepted, notes));
                    }
                }
                None if idle_since.elapsed() >= idle_limit => {
                    notes.append(&mut ch.drain_notes());
                    return Ok(self.summary(RunStop::Idle, notes));
                }
                None => {}
            }
            notes.append(&mut ch.drain_notes());
        }
    }

    /// 当前局号（切局键）
    pub fn epoch_key(&self) -> Option<u64> {
        self.epoch
    }

    /// 已处理帧数与已记录决策数
    pub fn counts(&self) -> (usize, usize) {
        (self.frames, self.decisions)
    }

    /// 最近一次被采纳的终局帧结果
    pub fn final_frame(&self) -> Option<&FinalFrameOutcome> {
        self.final_frame.as_ref()
    }

    /// 回合帧：通用段解析 → 重建局面 → 路由决策 → 记录（可挂运气分）
    fn step_turn(&mut self, contents: &str) -> Result<StepOutcome> {
        let status = parse_base_status(contents)?;
        let epoch = status.epoch_key();
        let FrameOutcome { game: base, notes } = status.to_base_game(&self.params)?;
        // 帧内事件由会话统一走事件决策（局面里的同一份不再结算，避免两处各跑一遍）
        let events = base.unresolved_events.clone();
        let mut game = (self.build)(base)?;

        if self.frames == 1 {
            self.fe.on_run_start(&RunStart {
                scenario: self.cfg.sim.scenario.clone(),
                trainer: self.cfg.sim.trainer.clone(),
                seed: self.cfg.sim.seed,
            });
        }
        if self.epoch != Some(epoch) {
            self.epoch = Some(epoch);
            self.fe.on_game_start(&GameStart {
                epoch_key: epoch,
                uma_id: status.uma_id,
                max_turn: game.max_turn(),
            });
        }

        let route = if status.source.as_deref() == Some(SOURCE_EVENT) {
            FrameRoute::Event
        } else {
            FrameRoute::Training
        };
        let ctx = StageCtx {
            turn: game.turn(),
            max_turn: game.max_turn(),
            stage: game.stage(),
        };
        self.fe.on_stage_begin(&ctx);
        let res: Result<bool> = match route {
            FrameRoute::Event => self.run_events(&mut game, &events),
            _ => game.run_stage(self.trainer, &mut self.rng).map(|()| false),
        };
        self.fe.on_stage_end();
        let multi = res?;

        let (decision, turn_luck) = self.record(route, multi, epoch, game.turn(), game.max_turn());
        Ok(StepOutcome {
            route,
            decision,
            turn_luck,
            final_frame: None,
            notes,
        })
    }

    /// 帧内事件逐个走事件决策；返回是否含「多选项」事件（决定是否记一条决策）
    fn run_events(&mut self, game: &mut G, events: &[EventData]) -> Result<bool> {
        let mut multi = false;
        for ev in events {
            if ev.player_select && ev.choices.len() > 1 {
                multi = true;
            }
            game.run_event(ev, self.trainer, &mut self.rng)?;
        }
        Ok(multi)
    }

    /// 取走本帧决策：事件帧仅「多选项」记且不挂运气分；训练帧按评分挂运气分
    fn record(
        &mut self,
        route: FrameRoute,
        multi: bool,
        epoch: u64,
        turn: i32,
        max_turn: i32,
    ) -> (Option<DecisionRecord>, Option<f64>) {
        if route == FrameRoute::Event && !multi {
            return (None, None);
        }
        let Some(src) = self.src else {
            return (None, None);
        };
        let Some(mut rec) = src.take_decision() else {
            return (None, None);
        };
        let luck = match route {
            FrameRoute::Training => self.scorer.attach(&mut rec, epoch, turn, max_turn),
            _ => None,
        };
        self.decisions += 1;
        self.fe.on_decision(&rec);
        (Some(rec), luck)
    }

    /// 终局帧：契约必填（解析失败即报错）→ 三道闸裁决
    fn step_final(&mut self, contents: &str) -> Result<StepOutcome> {
        let frame = FinalScoreFrame::parse(contents)?;
        let outcome = self.adjudicate(frame)?;
        Ok(StepOutcome {
            route: FrameRoute::FinalScore,
            decision: None,
            turn_luck: None,
            final_frame: Some(outcome),
            notes: vec![],
        })
    }

    /// 闸2 数值可用 + 闸3 局号匹配；通过则算分并下发 `run_end`
    fn adjudicate(&mut self, frame: FinalScoreFrame) -> Result<FinalFrameOutcome> {
        let mut out = FinalFrameOutcome {
            frame: frame.clone(),
            accepted: false,
            reject_reason: None,
            score: None,
            rank: None,
        };
        if !frame.is_usable() {
            out.reject_reason = Some("五维或五维上限全零".to_string());
            return Ok(out);
        }
        match self.epoch {
            Some(epoch) if frame.matches_epoch(epoch) => {}
            Some(epoch) => {
                out.reject_reason = Some(format!("局号不匹配：帧 {:?}，当前 {epoch}", frame.epoch_key()));
                return Ok(out);
            }
            None => {
                out.reject_reason = Some("尚未收到回合帧，无当前局号可比对".to_string());
                return Ok(out);
            }
        }
        out.accepted = true;
        out.score = Some(frame.calc_score()?);
        out.rank = Some(frame.rank_name()?);
        self.final_frame = Some(out.clone());
        self.fe.on_run_end(&end_summary(&out, self.frames, self.decisions));
        Ok(out)
    }

    /// 组装会话汇总
    fn summary(&self, stop: RunStop, notes: Vec<ChannelNote>) -> SessionSummary {
        SessionSummary {
            stop,
            frames: self.frames,
            decisions: self.decisions,
            epoch_key: self.epoch,
            final_frame: self.final_frame.clone(),
            notes,
        }
    }
}

/// 会话终局汇总：口径为终局帧（育成结束·点技能前），与离线 `Runner` 的模拟终局不同源
fn end_summary(out: &FinalFrameOutcome, frames: usize, decisions: usize) -> Summary {
    Summary {
        score: out.score.unwrap_or(0),
        pt: out.frame.skill_pt,
        rank: out.rank.clone().unwrap_or_default(),
        five_status: out.frame.five_status,
        turn: out.frame.turn,
        stages: frames,
        decisions,
    }
}
