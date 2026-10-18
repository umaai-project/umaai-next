//! 运行编排：驱动一局、按阶段下发事件、汇总终局
//!
//! [`Runner`] 只做编排，不认识任何剧本：一局怎么推进由 [`Game`] 契约决定，决策由
//! [`Trainer`] 给出，决策上下文经 [`DecisionSource`] 可选注入。回合推进顺序与
//! `Game::run_full_game` 完全一致（先跑当前阶段，再 `next`，最后 `on_simulation_end`），
//! 因此经本层跑出的终局与直接调用该 provided 方法逐位相同。

use anyhow::{Context, Result};
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use umaai_core::Data;
use umasim::game::{Game, Trainer};

use crate::config::Config;
use crate::decision::DecisionSource;
use crate::frontend::{ErrorEvent, ErrorKind, Frontend, GameStart, RunStart, StageCtx, UmaStatus};

/// 一局终局汇总
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// 终局评分（`Uma::calc_score`）
    pub score: i32,
    /// 终局 PT
    pub pt: i32,
    /// 评分档次名（查静态数据的分界表）
    pub rank: String,
    /// 终局五维
    pub five_status: [i32; 5],
    /// 终局回合（0 基）
    pub turn: i32,
    /// 走过的阶段数
    pub stages: usize,
    /// 捕获到的决策数
    pub decisions: usize,
}

/// 运行器：持配置、随机流、前端与计数
pub struct Runner<'a> {
    cfg: &'a Config,
    fe: &'a mut dyn Frontend,
    rng: StdRng,
    stages: usize,
    decisions: usize,
}

impl<'a> Runner<'a> {
    /// 建立运行器；要求全局静态数据已初始化（宿主先调 `Data::init`）
    pub fn new(cfg: &'a Config, fe: &'a mut dyn Frontend) -> Result<Self> {
        Data::try_get().context("全局静态数据未初始化：宿主应先调用 umaai_core::Data::init(data_root)")?;
        Ok(Self {
            cfg,
            fe,
            // 种子来自配置：同一配置重复运行必须逐位一致
            rng: StdRng::seed_from_u64(cfg.sim.seed),
            stages: 0,
            decisions: 0,
        })
    }

    /// 驱动一局到底，返回终局汇总
    ///
    /// `src` 为可选的决策上下文提供方：不提供时只统计决策次数，记录里没有候选与评分。
    pub fn run_game<G: Game, T: Trainer<G>>(
        &mut self,
        mut game: G,
        trainer: &T,
        src: Option<&dyn DecisionSource>,
    ) -> Result<Summary> {
        // 规则主种子：本局种子。引擎的局面/事件流与剧本自持的策略流都按它派生
        game.set_rule_master(self.cfg.sim.seed);
        self.fe.on_run_start(&RunStart {
            scenario: self.cfg.sim.scenario.clone(),
            trainer: self.cfg.sim.trainer.clone(),
            seed: self.cfg.sim.seed,
        });
        self.fe.on_game_start(&GameStart {
            epoch_key: game.uma().uma_id as u64,
            uma_id: game.uma().uma_id,
            max_turn: game.max_turn(),
        });
        match self.drive(&mut game, trainer, src) {
            Ok(()) => match self.summarize(&game) {
                Ok(sum) => {
                    self.fe.on_run_end(&sum);
                    Ok(sum)
                }
                Err(err) => {
                    self.report(ErrorKind::Internal, &err);
                    Err(err)
                }
            },
            Err(err) => {
                self.report(ErrorKind::Game, &err);
                Err(err)
            }
        }
    }

    /// 回合推进：与 `Game::run_full_game` 同序
    fn drive<G: Game, T: Trainer<G>>(
        &mut self,
        game: &mut G,
        trainer: &T,
        src: Option<&dyn DecisionSource>,
    ) -> Result<()> {
        self.stage(game, trainer, src)?;
        while game.next() {
            self.stage(game, trainer, src)?;
        }
        game.on_simulation_end(trainer, &mut self.rng)?;
        Ok(())
    }

    /// 单个阶段：事件成对下发，错误路径也闭合
    fn stage<G: Game, T: Trainer<G>>(
        &mut self,
        game: &mut G,
        trainer: &T,
        src: Option<&dyn DecisionSource>,
    ) -> Result<()> {
        let ctx = StageCtx {
            turn: game.turn(),
            max_turn: game.max_turn(),
            stage: game.stage(),
        };
        self.fe.on_stage_begin(&ctx);
        let res = game.run_stage(trainer, &mut self.rng);
        self.fe.on_stage_end();
        res?;
        self.stages += 1;
        // 局面摘要在阶段闭合之后下发：这里的 `?` 不破坏 on_stage_begin / on_stage_end 的成对
        let status = Self::status(game)?;
        self.fe.on_uma_status(&status);
        // 阶段边界取走决策上下文：实现方按「取走即清空」语义交付
        if let Some(src) = src
            && let Some(rec) = src.take_decision()
        {
            self.decisions += 1;
            self.fe.on_decision(&rec);
        }
        Ok(())
    }

    /// 阶段结束时的局面摘要（只带剧本无关字段）
    fn status<G: Game>(game: &G) -> Result<UmaStatus> {
        let uma = game.uma();
        Ok(UmaStatus {
            turn: game.turn(),
            max_turn: game.max_turn(),
            stage: game.stage(),
            vital: uma.vital,
            max_vital: uma.max_vital,
            motivation: uma.motivation,
            five_status: uma.five_status,
            pt: uma.total_pt()?,
            total_hints: uma.total_hints,
        })
    }

    /// 终局汇总（评分口径取静态数据的分界表）
    fn summarize<G: Game>(&self, game: &G) -> Result<Summary> {
        let uma = game.uma();
        let score = uma.calc_score()?;
        Ok(Summary {
            score,
            pt: uma.total_pt()?,
            rank: Data::get()?.consts.get_rank_name(score).to_string(),
            five_status: uma.five_status,
            turn: game.turn(),
            stages: self.stages,
            decisions: self.decisions,
        })
    }

    /// 下发错误事件（引擎不打印，文案由前端决定）
    fn report(&mut self, kind: ErrorKind, err: &anyhow::Error) {
        self.fe.on_error(&ErrorEvent {
            kind,
            detail: format!("{err:#}"),
        });
    }
}
