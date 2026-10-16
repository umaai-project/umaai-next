//! 呈现样例：把引擎事件渲染成人读文本
//!
//! [`PrintFrontend`] 是「一局到底」（[`umaai::Runner`]）的呈现参考，也可直接喂给
//! 帧驱动会话（[`umaai::Session`]）——两者都只经 [`Frontend`] 回调出数据。
//!
//! 三条纪律：
//!
//! - **只往注入的 writer 写**，从不直接 `println!`，因此重定向、测试断言与剧本仓
//!   复用都不需要改本模块。
//! - **错误不在这里打印**：引擎在返回 `Err` 之前已经下发过 `on_error`，本层的样例选择
//!   让入口层统一打印一次 `Err` 的因果链，避免同一错误两份输出。下游要分类呈现时再
//!   覆写 `on_error`。
//! - **回合行按回合合并**：阶段粒度太碎（基础局一回合多个阶段），默认档只在回合切换时
//!   落盘上一回合的末态；要看阶段粒度用 [`Verbosity::Verbose`]。

use std::io::Write;

use umaai::{DecisionRecord, Frontend, GameStart, RunStart, Summary, UmaStatus};
use umaai_core::gamedata::GameConstants;

use crate::render::explain;
use crate::render::panel;
use crate::render::style::{Styler, Tone, motivation_tone, vital_tone};
use crate::render::table::Grid;
use crate::render::term;

/// 输出详细度
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Verbosity {
    /// 只出终局块
    Quiet,
    /// 每回合一行（回合末局面）+ 终局块
    #[default]
    Normal,
    /// 每阶段一行 + 决策明细 + 终局块
    Verbose,
}

/// 呈现选项
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrintOptions {
    /// 详细度
    pub verbosity: Verbosity,
    /// 呈现宽度（列）
    pub width: usize,
    /// 是否着色
    pub color: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            verbosity: Verbosity::default(),
            width: term::FALLBACK_WIDTH,
            color: false,
        }
    }
}

/// print 前端：引擎事件的终端呈现（样例实现）
pub struct PrintFrontend<W: Write> {
    out: W,
    opts: PrintOptions,
    styler: Styler,
    consts: &'static GameConstants,
    /// 已落盘的上一回合
    last_turn: Option<i32>,
    /// 已攒下、尚未落盘的回合末局面
    pending: Option<UmaStatus>,
    /// 一局的最大回合数（来源于 [`GameStart`]，终局块用）
    max_turn: i32,
    write_failed: bool,
}

impl<W: Write> PrintFrontend<W> {
    /// 建立前端；`consts` 取自已初始化的静态数据（轴名与干劲名的唯一来源）
    pub fn new(out: W, opts: PrintOptions, consts: &'static GameConstants) -> Self {
        Self {
            out,
            opts,
            styler: Styler::new(opts.color),
            consts,
            last_turn: None,
            pending: None,
            max_turn: 0,
            write_failed: false,
        }
    }

    /// 是否有写入失败（下游提前关闭输出，如 `| head`）
    #[must_use]
    pub fn write_failed(&self) -> bool {
        self.write_failed
    }

    /// 写一行（失败只记标志，不 panic——`Frontend` 回调没有错误通道）
    fn line(&mut self, text: &str) {
        if writeln!(self.out, "{text}").is_err() {
            self.write_failed = true;
        }
    }

    /// 落盘已攒下的回合末局面
    fn flush_turn(&mut self) {
        if let Some(status) = self.pending.take() {
            let text = self.turn_line(&status);
            self.line(&text);
        }
    }

    /// 回合行：`T0   体 100/100 干劲 普通 速 12 耐 10 … PT 0`
    fn turn_line(&self, status: &UmaStatus) -> String {
        format!("T{:<3} {}", status.turn, self.status_brief(status))
    }

    /// 阶段行：`T0   stage#2  体 100/100 …`
    fn stage_line(&self, status: &UmaStatus) -> String {
        format!("T{:<3} {:<8} {}", status.turn, status.stage, self.status_brief(status))
    }

    /// 局面摘要（体力 / 干劲 / 五维 / PT），回合行与阶段行共用
    fn status_brief(&self, status: &UmaStatus) -> String {
        let vital = self.styler.paint(
            vital_tone(status.vital, status.max_vital),
            &format!("{}/{}", status.vital, status.max_vital),
        );
        let mot_name = explain::motivation(self.consts, status.motivation);
        let motivation = self.styler.paint(motivation_tone(status.motivation), mot_name);
        let five = explain::five_status(self.consts, &status.five_status);
        let pt = self.styler.paint(Tone::Accent, &status.pt.to_string());
        format!("体 {vital} 干劲 {motivation} {five} PT {pt}")
    }

    /// 决策行（`Verbose` 档）：`→ [train] 训练：耐力（候选 5）`
    fn decision_line(&self, rec: &DecisionRecord) -> String {
        let arrow = self.styler.paint(Tone::Accent, "→");
        let kind = self.styler.paint(Tone::Muted, &rec.kind);
        if rec.candidates.is_empty() {
            format!("    {arrow} [{kind}] {}", rec.action_desc)
        } else {
            format!(
                "    {arrow} [{kind}] {}（候选 {}）",
                rec.action_desc,
                rec.candidates.len()
            )
        }
    }

    /// 终局块：键值摘要 + 五维表
    fn final_block(&self, sum: &Summary) -> String {
        let rows = vec![
            ("评分".to_string(), self.ranked_score(sum)),
            ("PT".to_string(), self.styler.paint(Tone::Accent, &sum.pt.to_string())),
            ("回合".to_string(), format!("{}/{}", sum.turn, self.max_turn)),
            ("阶段".to_string(), sum.stages.to_string()),
            ("决策".to_string(), sum.decisions.to_string()),
        ];
        let mut grid = Grid::new(self.axis_names());
        grid.push(sum.five_status.iter().map(i32::to_string));
        format!(
            "{}\n{}\n{}",
            panel::title("终局", self.opts.width),
            panel::key_values(&rows),
            grid.render(self.opts.width)
        )
    }

    /// 评分与档次名（档次名是静态数据查表结果）
    fn ranked_score(&self, sum: &Summary) -> String {
        format!("{} {}", sum.score, self.styler.paint(Tone::Accent, &sum.rank))
    }

    /// 五维轴名（静态数据缺失时退化为下标）
    fn axis_names(&self) -> Vec<String> {
        (0..5)
            .map(|i| {
                self.consts
                    .train_names
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| format!("#{i}"))
            })
            .collect()
    }
}

impl<W: Write> Frontend for PrintFrontend<W> {
    fn on_run_start(&mut self, ctx: &RunStart) {
        if self.opts.verbosity == Verbosity::Quiet {
            return;
        }
        let head = panel::title("运行", self.opts.width);
        let line = format!(
            "剧本 {} 训练员 {} 种子 {}",
            self.styler.paint(Tone::Normal, &ctx.scenario),
            ctx.trainer,
            ctx.seed
        );
        self.line(&head);
        self.line(&line);
    }

    fn on_game_start(&mut self, ctx: &GameStart) {
        self.max_turn = ctx.max_turn;
        if self.opts.verbosity != Verbosity::Verbose {
            return;
        }
        let line = format!("一局开始 uma={} 回合 {}/{}", ctx.uma_id, 0, ctx.max_turn);
        self.line(&line);
    }

    fn on_uma_status(&mut self, status: &UmaStatus) {
        match self.opts.verbosity {
            Verbosity::Quiet => {}
            Verbosity::Verbose => {
                let text = self.stage_line(status);
                self.line(&text);
            }
            Verbosity::Normal => {
                // 回合切换即落盘上一回合的末态（阶段粒度对默认档太碎）
                if self.last_turn.is_some_and(|turn| turn != status.turn) {
                    self.flush_turn();
                }
                self.last_turn = Some(status.turn);
                self.pending = Some(*status);
            }
        }
    }

    fn on_decision(&mut self, rec: &DecisionRecord) {
        if self.opts.verbosity != Verbosity::Verbose {
            return;
        }
        let text = self.decision_line(rec);
        self.line(&text);
    }

    fn on_run_end(&mut self, sum: &Summary) {
        self.flush_turn();
        let block = self.final_block(sum);
        self.line(&block);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata;
    use umasim::game::StageId;

    /// 固定局面摘要
    fn status(turn: i32) -> UmaStatus {
        UmaStatus {
            turn,
            max_turn: 77,
            stage: StageId::new(2),
            vital: 100,
            max_vital: 100,
            motivation: 3,
            five_status: [12, 10, 10, 10, 10],
            pt: 0,
            total_hints: 0,
        }
    }

    /// 跑一局只给终局的夹具（无颜色、固定宽度）
    fn frontend(verbosity: Verbosity) -> PrintFrontend<Vec<u8>> {
        PrintFrontend::new(
            Vec::new(),
            PrintOptions {
                verbosity,
                width: 60,
                color: false,
            },
            testdata::consts(),
        )
    }

    /// 层1：默认档按回合合并，两个回合只落一行，末回合在终局前补落
    #[test]
    fn test_normal_merges_by_turn() {
        let mut fe = frontend(Verbosity::Normal);
        fe.on_uma_status(&status(0));
        fe.on_uma_status(&status(0));
        fe.on_uma_status(&status(1));
        let out = String::from_utf8(fe.out).expect("UTF-8");
        let turn_lines = out.lines().filter(|l| l.starts_with("T0 ")).count();
        assert_eq!(turn_lines, 1, "同回合的多阶段应合并成一行: {out:?}");
        assert!(out.contains("体 100/100"));
    }

    /// 层1：静默档不落任何过程行
    #[test]
    fn test_quiet_only_final() {
        let mut fe = frontend(Verbosity::Quiet);
        fe.on_run_start(&RunStart {
            scenario: "basic".into(),
            trainer: "random".into(),
            seed: 42,
        });
        fe.on_uma_status(&status(0));
        let out = String::from_utf8(fe.out).expect("UTF-8");
        assert!(out.is_empty(), "静默档不应有过程输出: {out:?}");
    }

    /// 层1：详细档每个阶段一行
    #[test]
    fn test_verbose_per_stage() {
        let mut fe = frontend(Verbosity::Verbose);
        fe.on_game_start(&GameStart {
            epoch_key: 1,
            uma_id: 101901,
            max_turn: 77,
        });
        fe.on_uma_status(&status(0));
        fe.on_uma_status(&status(0));
        let out = String::from_utf8(fe.out).expect("UTF-8");
        assert_eq!(out.lines().filter(|l| l.contains("stage#2")).count(), 2);
    }

    /// 层1：终局块含键值摘要与五维表
    #[test]
    fn test_final_block() {
        let mut fe = frontend(Verbosity::Normal);
        fe.on_run_end(&Summary {
            score: 12345,
            pt: 350,
            rank: "A+".into(),
            five_status: [900, 800, 700, 600, 500],
            turn: 77,
            stages: 468,
            decisions: 0,
        });
        let out = String::from_utf8(fe.out).expect("UTF-8");
        assert!(out.contains("12345"), "{out}");
        assert!(out.contains("A+"));
        assert!(out.contains("速"), "五维表应带轴名");
        assert!(out.contains("900"));
    }
}
