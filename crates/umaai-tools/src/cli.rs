//! 入口层：参数解析 + 子命令分发
//!
//! 入口逻辑落在库里而不在 bin：集成测试可以直接调 [`run_from_args`]
//! （宿主目录可注入的形态是 [`run_from_args_with_host_dir`]）并注入输出目标；
//! bin 只收参数、报告失败、给退出码（与 `umaai-cli` 同法）。
//!
//! 三个子命令共用一条数据装配链：配置层（默认层 + 宿主目录同名覆盖层，宿主目录可注入）
//! 取参数、静态数据按 `--data-root` 载入，故工具看到的数值与单局入口**同源**。
//!
//! 选项解析用 `lexopt`（只做「取到令牌」这一件事，不生成代码、不替调用方规定结构）：
//! 取值语法、`--opt=val`、短选项与「不认识就报错」都由它兜住，本层只负责**子命令归属
//! 校验**与中文错误文案。

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail, ensure};
use lexopt::prelude::*;
use umaai::Config;
use umaai_core::Data;
use umaai_core::stats::Correct;

use crate::baseline::{self, Case, Difference};
use crate::cardtest;
use crate::deck::{
    self, DEFAULT_CARD_ID_MIN, DEFAULT_EXCLUDE, DEFAULT_REFERENCE_DECK, DEFAULT_SCAN_RUNS, DEFAULT_TOP_K,
    DEFAULT_TRAINING_SLOTS, DeckSpec, FULL_RANK, FixedCard, Metric, PruneMode, ScanSpec, Stage,
};
use crate::{batch, compare, probe, report};

/// 数据根默认值（相对当前目录；发布形态的定位留打包阶段定）
pub const DEFAULT_DATA_ROOT: &str = "gamedata";

/// `DECK_RANK` 机读行后附带的前几名明细条数
const RANK_TOP_LINES: usize = 3;

/// `cardtest --greedy` 的步数上限缺省值
const DEFAULT_GREEDY_STEPS: usize = 5;

/// 基线目录默认值（相对 workspace 根）
pub const DEFAULT_BASELINE_DIR: &str = "crates/umaai-tests/baselines";

/// 子命令与各自的选项
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    /// 批量跑局（`--runs` / `--seed` 缺省取配置层）
    Bench {
        /// 局数
        runs: Option<u32>,
        /// 起始种子
        seed: Option<u64>,
    },
    /// 固定根搜索成本探针（缺省取配置层）
    Probe {
        /// 决定推进路径与搜索随机流的种子
        seed: Option<u64>,
        /// 每候选搜索次数
        search_n: Option<usize>,
    },
    /// 基线校验 / 重录
    Baseline {
        /// 基线目录
        dir: PathBuf,
        /// 只处理指定基线名（可重复；空 = 全部）
        only: Vec<String>,
        /// 重录期望值与元信息
        update: bool,
    },
    /// 分支推进对照（复盘用；计划自持）
    Compare {
        /// 对照计划 JSON 路径
        plan: PathBuf,
    },
    /// 组卡工具（卡池 / 构成 / 逐类型实测扫描 / 排序）
    Deck {
        /// 自持实验计划 JSON（给了它就不再接受任何实验开关）
        plan: Option<PathBuf>,
        /// 命令行给出的实验变量（`plan` 为 `Some` 时不参与）
        args: DeckArgs,
    },
    /// 测卡工具（固定基准卡组、只换一个槽位，显式候选列表）
    Cardtest(CardTestArgs),
}

/// `cardtest` 子命令的选项
#[derive(Debug, Clone, PartialEq)]
pub struct CardTestArgs {
    /// 基准卡组（idrank；必给）
    pub base: Vec<u32>,
    /// 变动槽位（单槽模式必给；`--greedy` 时不给）
    pub slot: Option<usize>,
    /// 候选 idrank（必给，可重复累加）
    pub cards: Vec<u32>,
    /// 逐槽贪婪（每步测「槽位 × 候选」，取最优且越过零者替换）
    pub greedy: bool,
    /// 贪婪的步数上限
    pub greedy_steps: Option<usize>,
    /// 每臂局数
    pub runs: Option<u32>,
    /// 起始种子（缺省取配置层 `sim.seed`）
    pub seed: Option<u64>,
    /// 观测轴
    pub metric: Option<String>,
    /// 目标分辨率（反推局数用）
    pub goal: Option<f64>,
    /// 多重比较校正口径
    pub correct: Option<Correct>,
    /// 允许候选与基准卡类型不同
    pub allow_cross_type: bool,
}

/// `deck` 子命令的选项（缺省值见 [`crate::deck`] 各常量）
#[derive(Debug, Clone, PartialEq)]
pub struct DeckArgs {
    /// 阶段
    pub stage: Stage,
    /// 卡池下限（`None` = 默认 30251，即 `cardId > 30250`）
    pub card_id_min: Option<u32>,
    /// 排除名单；`Some(vec![])` = 显式清空
    pub exclude: Option<Vec<u32>>,
    /// 参考卡组（idrank；`None` = GA 万能骨架）
    pub seed_deck: Option<Vec<u32>>,
    /// 每臂局数（扫描用）
    pub runs: Option<u32>,
    /// 起始种子（缺省取配置层 `sim.seed`）
    pub seed: Option<u64>,
    /// 观测轴
    pub metric: Option<String>,
    /// 每类型保留候选数（构成枚举用）
    pub top_k: Option<usize>,
    /// 卡组槽位数
    pub slots: Option<usize>,
    /// 必带卡（槽位 + idrank；不参与扫描，只随参考卡组进组）
    pub fixed: Vec<FixedCard>,
    /// 单类型张数上限
    pub max_per_type: Option<usize>,
    /// 种类数下限
    pub min_types: Option<usize>,
    /// 剪枝口径（`rank` / `all` 用；缺省 measured）
    pub prune: Option<PruneMode>,
    /// 粗筛每臂局数（`rank` 用）
    pub screen_runs: Option<u32>,
    /// 进精评的候选数（`rank` 用）
    pub fine_count: Option<usize>,
    /// 验证种子（`rank` 用；缺省独立于 `--seed`）
    pub holdout_seed: Option<u64>,
    /// 进验证的候选数（`rank` 用）
    pub holdout_count: Option<usize>,
    /// 目标分辨率（反推局数用）
    pub goal: Option<f64>,
    /// 多重比较校正口径（`scan` / `all` 用；缺省不校正）
    pub correct: Option<Correct>,
}

impl From<&deck::Plan> for DeckArgs {
    /// 计划 → 命令行口径：计划里缺省的可选项留 `None`，由下游各自取工具常量
    fn from(plan: &deck::Plan) -> Self {
        Self {
            stage: plan.stage,
            card_id_min: plan.card_id_min,
            exclude: plan.exclude.clone(),
            seed_deck: plan.seed_deck.clone(),
            runs: plan.runs,
            seed: Some(plan.seed),
            metric: plan.metric.map(|metric| metric_name(metric).to_string()),
            top_k: plan.top_k,
            slots: plan.slots,
            fixed: plan.fixed.clone(),
            max_per_type: plan.max_per_type,
            min_types: plan.min_types,
            prune: plan.prune,
            screen_runs: plan.screen_runs,
            fine_count: plan.fine_count,
            holdout_seed: plan.holdout_seed,
            holdout_count: plan.holdout_count,
            goal: plan.goal,
            correct: plan.correct,
        }
    }
}

/// 命令行参数
#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    /// 子命令（`None` = 只打用法）
    pub cmd: Option<Cmd>,
    /// 数据根（只读输入）
    pub data_root: PathBuf,
    /// 呈现宽度（`None` = 探测终端）
    pub width: Option<usize>,
    /// 只打机读行（脚本用）
    pub quiet: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            cmd: None,
            data_root: PathBuf::from(DEFAULT_DATA_ROOT),
            width: None,
            quiet: false,
        }
    }
}

impl Args {
    /// 用法文本
    #[must_use]
    pub fn usage() -> &'static str {
        "umaai-tools —— 探针 / 基准 / 基线（开发工具，不进发布产物）

用法: umaai-tools <子命令> [选项]      （选项可放在子命令前后）

子命令:
  bench      固定种子批量跑局，输出评分分布与吞吐
  probe      固定根搜索成本探针（首个训练决策点）
  baseline   基线校验；--update 重录期望值与元信息
  compare    分支推进对照（复盘：探测动作 vs 默认一选）
  deck       组卡工具（卡池 / 构成 / 逐类型实测扫描 / 排序）
  cardtest   测卡工具（固定基准卡组、只换一个槽位，测单卡边际）

公共选项:
  --data-root <路径>  数据根（默认 gamedata，相对当前目录；内含默认层配置）
  --width <列数>      呈现宽度（默认探测终端；最小 40）
  --quiet             只打机读行
  -h, --help          打印本用法

bench 选项:
  --runs <N>          局数（默认取配置 sim.runs）
  --seed <S>          起始种子（默认取配置 sim.seed；第 i 局用 seed + i）

probe 选项:
  --seed <S>          搜索随机种子（默认取配置 sim.seed）
  --search-n <N>      每候选搜索次数（默认取配置 search.search_n）

baseline 选项:
  --dir <路径>        基线目录（默认 crates/umaai-tests/baselines）
  --only <名字>       只处理指定基线（可重复）
  --update            重录（跑法不变，只刷新期望值与元信息）

compare 选项:
  --plan <路径>       对照计划 JSON（必给）：seed / turn / action / turns / note
                      turn = 探测回合坐标，action = 该回合候选下标，turns = 观测回合数

deck 选项（口径见 docs/plan/tools_deck_cardtest.md）:
  --stage <阶段>      scan（逐类型实测扫描，默认）/ compose（构成枚举 + 静态代表卡）
                      rank（每构成一副代表卡组：粗筛→精评→独立种子集验证）/ all（scan+rank）
  --card-id-min <N>   卡池下限（默认 30251，即 cardId > 30250）
  --exclude <id,id>   排除名单（默认 30257；给了即整体覆盖，可重复累加）
  --no-exclude        清空排除名单
  --seed-deck <idr,…> 参考卡组（idrank 逗号分隔；默认 GA 万能骨架）
  --runs <N>          每臂局数（默认 1000）
  --seed <S>          起始种子（默认取配置 sim.seed）
  --metric <轴>       score（默认）/ pt / five
  --top-k <N>         每类型保留候选数（默认 5）
  --slots <N>         卡组总槽位数（默认 5 张训练卡 + 必带卡数）
  --fixed <槽=idrank> 必带卡（可重复；拼进参考卡组，不参与扫描，如 5=303054）
  --max-per-type <N>  单类型张数上限（默认不限；剧本可 5 速）
  --min-types <N>     种类数下限（默认不限）
  --prune <口径>      static（静态面板分）/ measured（实测扫描排名，默认）
  --screen-runs <N>   rank 粗筛每臂局数（默认 20）
  --fine-count <N>    rank 进精评的候选数（默认 10）
  --holdout-seed <S>  rank 验证种子（默认 seed + 1000000，必须与 --seed 不同）
  --holdout-count <N> rank 进验证的候选数（默认 3）
  --goal <分值>       反推「所需局数」的目标分辨率（默认 25）
  --correct <口径>    none（默认）/ bonferroni / holm：多重比较校正（只对 scan / all 有意义）
  --plan <路径>       自持实验计划 JSON：实验变量只认计划（stage 与 seed 必给，
                      其余缺省取工具常量、不读配置层）；再给任何实验开关即报错

cardtest 选项（固定基准卡组、只换一个槽位或逐槽贪婪）:
  --base <idr,…>      基准卡组（idrank 逗号分隔，必给，至少 5 张）
  --slot <N>          变动槽位（单槽模式必给，0 基；与 --greedy 互斥）
  --cards <idr,…>     候选 idrank（必给，可重复累加；可含不同突破档）
  --greedy            逐槽贪婪：每步测「槽位 × 候选」，取最正向且越过零者替换一个槽位
  --greedy-steps <N>  贪婪步数上限（默认 5；需 --greedy）
  --allow-cross-type  允许候选与基准卡类型不同（默认拒绝：类型变了测的是构成效应）
  --runs <N>          每臂局数（默认 1000）
  --seed <S>          起始种子（默认取配置 sim.seed）
  --metric <轴>       score（默认）/ pt / five
  --goal <分值>       反推「所需局数」的目标分辨率（默认 25）
  --correct <口径>    none（默认）/ bonferroni / holm：多重比较校正（族 = 本批候选）"
    }

    /// 解析参数（`argv` 含程序名，与 `std::env::args` 一致）
    ///
    /// 选项可在子命令前后任意位置；**子命令归属在解析末尾统一校验**，故「某个子命令不
    /// 认识的选项」总能被指出来，而不是靠书写顺序碰巧报错。
    pub fn parse(argv: &[String]) -> Result<Self> {
        let mut raw = Raw::default();
        let mut parser = lexopt::Parser::from_iter(argv.iter().cloned());
        while let Some(arg) = parser.next().map_err(bad_arg)? {
            match arg {
                Short('h') | Long("help") => raw.help = true,
                Value(name) if raw.sub.is_none() => raw.sub = Some(name.string().map_err(bad_arg)?),
                other => {
                    // 走到这里说明「不是帮助、也不是首个位置参数」，即一个选项：记一笔，
                    // 供「给了选项却没给子命令」判定用
                    raw.given = true;
                    match other {
                        Long("data-root") => raw.data_root = Some(PathBuf::from(text(&mut parser, "--data-root")?)),
                        Long("width") => {
                            let raw_width = text(&mut parser, "--width")?;
                            let cols: usize = raw_width
                                .parse()
                                .map_err(|_| anyhow!("--width 需要正整数，收到 \"{raw_width}\""))?;
                            if cols < report::MIN_WIDTH {
                                bail!("--width 必须 ≥ {}（更窄会把表格里的 CJK 切坏）", report::MIN_WIDTH);
                            }
                            raw.width = Some(cols);
                        }
                        Long("quiet") => raw.quiet = true,
                        Long("runs") => raw.runs = Some(num(&mut parser, "--runs")?),
                        Long("seed") => raw.seed = Some(num(&mut parser, "--seed")?),
                        Long("search-n") => raw.search_n = Some(num(&mut parser, "--search-n")?),
                        Long("dir") => raw.dir = Some(PathBuf::from(text(&mut parser, "--dir")?)),
                        Long("only") => raw.only.push(text(&mut parser, "--only")?),
                        Long("update") => raw.update = true,
                        Long("plan") => raw.plan = Some(PathBuf::from(text(&mut parser, "--plan")?)),
                        Long("stage") => raw.stage = Some(text(&mut parser, "--stage")?),
                        Long("card-id-min") => raw.card_id_min = Some(num(&mut parser, "--card-id-min")?),
                        Long("exclude") => {
                            let list = text(&mut parser, "--exclude")?;
                            raw.exclude.extend(id_list(&list, "--exclude")?);
                        }
                        Long("no-exclude") => raw.no_exclude = true,
                        Long("seed-deck") => {
                            let list = text(&mut parser, "--seed-deck")?;
                            raw.seed_deck.extend(id_list(&list, "--seed-deck")?);
                        }
                        Long("metric") => raw.metric = Some(text(&mut parser, "--metric")?),
                        Long("top-k") => raw.top_k = Some(num(&mut parser, "--top-k")?),
                        Long("slots") => raw.slots = Some(num(&mut parser, "--slots")?),
                        Long("fixed") => {
                            let pair = text(&mut parser, "--fixed")?;
                            raw.fixed.push(fixed_card(&pair, "--fixed")?);
                        }
                        Long("prune") => raw.prune = Some(text(&mut parser, "--prune")?),
                        Long("screen-runs") => raw.screen_runs = Some(num(&mut parser, "--screen-runs")?),
                        Long("fine-count") => raw.fine_count = Some(num(&mut parser, "--fine-count")?),
                        Long("holdout-seed") => raw.holdout_seed = Some(num(&mut parser, "--holdout-seed")?),
                        Long("holdout-count") => raw.holdout_count = Some(num(&mut parser, "--holdout-count")?),
                        Long("goal") => raw.goal = Some(number(&mut parser, "--goal")?),
                        Long("base") => {
                            let list = text(&mut parser, "--base")?;
                            raw.base.extend(id_list(&list, "--base")?);
                        }
                        Long("slot") => raw.slot = Some(num(&mut parser, "--slot")?),
                        Long("cards") => {
                            let list = text(&mut parser, "--cards")?;
                            raw.cards.extend(id_list(&list, "--cards")?);
                        }
                        Long("allow-cross-type") => raw.allow_cross_type = true,
                        Long("greedy") => raw.greedy = true,
                        Long("greedy-steps") => raw.greedy_steps = Some(num(&mut parser, "--greedy-steps")?),
                        Long("correct") => raw.correct = Some(text(&mut parser, "--correct")?),
                        Long("max-per-type") => raw.max_per_type = Some(num(&mut parser, "--max-per-type")?),
                        Long("min-types") => raw.min_types = Some(num(&mut parser, "--min-types")?),
                        other => return Err(bad_arg(other.unexpected())),
                    }
                }
            }
        }
        raw.finish()
    }
}

/// 解析期的原始取值（子命令归属未校验）
#[derive(Default)]
struct Raw {
    /// 子命令名（首个位置参数）
    sub: Option<String>,
    /// 是否要求只打用法
    help: bool,
    /// 是否出现过子命令专属选项（用来区分「空参数」与「给了选项却没给子命令」）
    given: bool,
    /// 数据根
    data_root: Option<PathBuf>,
    /// 呈现宽度
    width: Option<usize>,
    /// 只打机读行
    quiet: bool,
    /// 局数
    runs: Option<u32>,
    /// 种子
    seed: Option<u64>,
    /// 每候选搜索次数
    search_n: Option<usize>,
    /// 基线目录
    dir: Option<PathBuf>,
    /// 只处理的基线名
    only: Vec<String>,
    /// 是否重录
    update: bool,
    /// 对照计划路径
    plan: Option<PathBuf>,
    /// deck：阶段名
    stage: Option<String>,
    /// deck：卡池下限
    card_id_min: Option<u32>,
    /// deck：排除名单（累加）
    exclude: Vec<u32>,
    /// deck：清空排除名单
    no_exclude: bool,
    /// deck：参考卡组（累加）
    seed_deck: Vec<u32>,
    /// deck：观测轴
    metric: Option<String>,
    /// deck：每类型保留候选数
    top_k: Option<usize>,
    /// deck：卡组槽位数
    slots: Option<usize>,
    /// deck：必带卡（槽位 = idrank）
    fixed: Vec<FixedCard>,
    /// deck：剪枝口径（static / measured）
    prune: Option<String>,
    /// deck：粗筛每臂局数
    screen_runs: Option<u32>,
    /// deck：进精评的候选数
    fine_count: Option<usize>,
    /// deck：验证种子
    holdout_seed: Option<u64>,
    /// deck：进验证的候选数
    holdout_count: Option<usize>,
    /// deck / cardtest：目标分辨率
    goal: Option<f64>,
    /// cardtest：基准卡组
    base: Vec<u32>,
    /// cardtest：变动槽位
    slot: Option<usize>,
    /// cardtest：候选列表
    cards: Vec<u32>,
    /// cardtest：允许跨类型
    allow_cross_type: bool,
    /// cardtest：逐槽贪婪
    greedy: bool,
    /// cardtest：贪婪步数上限
    greedy_steps: Option<usize>,
    /// deck / cardtest：多重比较校正口径
    correct: Option<String>,
    /// deck：单类型张数上限
    max_per_type: Option<usize>,
    /// deck：种类数下限
    min_types: Option<usize>,
}

impl Raw {
    /// 归位到具体子命令，并校验选项归属
    fn finish(self) -> Result<Args> {
        let args = Args {
            cmd: None,
            data_root: self
                .data_root
                .clone()
                .unwrap_or_else(|| PathBuf::from(DEFAULT_DATA_ROOT)),
            width: self.width,
            quiet: self.quiet,
        };
        if self.help {
            return Ok(args);
        }
        let Some(name) = self.sub.clone() else {
            if self.given || self.tail_given() {
                bail!("缺少子命令（用 -h 看用法）");
            }
            return Ok(args);
        };
        let cmd = match name.as_str() {
            "bench" => {
                let mut bad = vec![
                    ("--search-n", self.search_n.is_some()),
                    ("--dir", self.dir.is_some()),
                    ("--only", !self.only.is_empty()),
                    ("--update", self.update),
                    ("--plan", self.plan.is_some()),
                ];
                bad.extend(self.deck_strays());
                bad.extend(self.cardtest_strays());
                bad.extend(self.tool_strays());
                let bad = stray(&bad);
                ensure!(bad.is_empty(), "bench 不认识 {}（用 -h 看用法）", bad.join(" / "));
                Cmd::Bench {
                    runs: self.runs,
                    seed: self.seed,
                }
            }
            "probe" => {
                let mut bad = vec![
                    ("--runs", self.runs.is_some()),
                    ("--dir", self.dir.is_some()),
                    ("--only", !self.only.is_empty()),
                    ("--update", self.update),
                    ("--plan", self.plan.is_some()),
                ];
                bad.extend(self.deck_strays());
                bad.extend(self.cardtest_strays());
                bad.extend(self.tool_strays());
                let bad = stray(&bad);
                ensure!(bad.is_empty(), "probe 不认识 {}（用 -h 看用法）", bad.join(" / "));
                Cmd::Probe {
                    seed: self.seed,
                    search_n: self.search_n,
                }
            }
            "baseline" => {
                let mut bad = vec![
                    ("--runs", self.runs.is_some()),
                    ("--seed", self.seed.is_some()),
                    ("--search-n", self.search_n.is_some()),
                    ("--plan", self.plan.is_some()),
                ];
                bad.extend(self.deck_strays());
                bad.extend(self.cardtest_strays());
                bad.extend(self.tool_strays());
                let bad = stray(&bad);
                ensure!(bad.is_empty(), "baseline 不认识 {}（用 -h 看用法）", bad.join(" / "));
                Cmd::Baseline {
                    dir: self.dir.unwrap_or_else(|| PathBuf::from(DEFAULT_BASELINE_DIR)),
                    only: self.only,
                    update: self.update,
                }
            }
            "compare" => {
                let mut bad = vec![
                    ("--runs", self.runs.is_some()),
                    ("--seed", self.seed.is_some()),
                    ("--search-n", self.search_n.is_some()),
                    ("--dir", self.dir.is_some()),
                    ("--only", !self.only.is_empty()),
                    ("--update", self.update),
                ];
                bad.extend(self.deck_strays());
                bad.extend(self.cardtest_strays());
                bad.extend(self.tool_strays());
                let bad = stray(&bad);
                ensure!(bad.is_empty(), "compare 不认识 {}（用 -h 看用法）", bad.join(" / "));
                let Some(plan) = self.plan else {
                    bail!("compare 需要 --plan <路径>（对照计划 JSON；用 -h 看用法）");
                };
                Cmd::Compare { plan }
            }
            "deck" => {
                let mut bad = vec![
                    ("--search-n", self.search_n.is_some()),
                    ("--dir", self.dir.is_some()),
                    ("--only", !self.only.is_empty()),
                    ("--update", self.update),
                ];
                bad.extend(self.cardtest_strays());
                let bad = stray(&bad);
                ensure!(bad.is_empty(), "deck 不认识 {}（用 -h 看用法）", bad.join(" / "));
                ensure!(
                    !(self.no_exclude && !self.exclude.is_empty()),
                    "--exclude 与 --no-exclude 不能同时给（用 -h 看用法）"
                );
                let stage = match &self.stage {
                    Some(raw) => Stage::parse(raw)?,
                    None => Stage::Scan,
                };
                if let Some(k) = self.top_k {
                    ensure!(k >= 1, "--top-k 必须 ≥ 1（收到 {k}）");
                }
                // 观测轴与校正口径在解析期就校验，错值不必等到跑局
                if let Some(raw) = &self.metric {
                    Metric::parse(raw)?;
                }
                let correct = parse_correct(self.correct.as_deref())?;
                let prune = match &self.prune {
                    Some(raw) => Some(PruneMode::parse(raw)?),
                    None => None,
                };
                // 计划自持：给了 --plan 就不再接受任何实验开关（不做「CLI 覆盖计划」的合并）
                if self.plan.is_some() {
                    let mut bad = self.deck_strays();
                    bad.extend(self.tool_strays());
                    bad.push(("--runs", self.runs.is_some()));
                    bad.push(("--seed", self.seed.is_some()));
                    let bad = stray(&bad);
                    ensure!(
                        bad.is_empty(),
                        "deck 给了 --plan 就不能再给 {}（实验变量只认计划，不做覆盖合并）",
                        bad.join(" / ")
                    );
                } else if correct.is_some() && !matches!(stage, Stage::Scan | Stage::All) {
                    bail!(
                        "--correct 只对 --stage scan / all 有意义（收到 --stage {}）",
                        stage.name()
                    );
                }
                // 排除名单三态：显式清空 / 显式覆盖 / 走默认
                let exclude = if self.no_exclude {
                    Some(Vec::new())
                } else if self.exclude.is_empty() {
                    None
                } else {
                    Some(self.exclude)
                };
                Cmd::Deck {
                    plan: self.plan,
                    args: DeckArgs {
                        stage,
                        card_id_min: self.card_id_min,
                        exclude,
                        seed_deck: if self.seed_deck.is_empty() {
                            None
                        } else {
                            Some(self.seed_deck)
                        },
                        runs: self.runs,
                        seed: self.seed,
                        metric: self.metric,
                        top_k: self.top_k,
                        slots: self.slots,
                        fixed: self.fixed,
                        max_per_type: self.max_per_type,
                        min_types: self.min_types,
                        prune,
                        screen_runs: self.screen_runs,
                        fine_count: self.fine_count,
                        holdout_seed: self.holdout_seed,
                        holdout_count: self.holdout_count,
                        goal: self.goal,
                        correct,
                    },
                }
            }
            "cardtest" => {
                let mut bad = vec![
                    ("--search-n", self.search_n.is_some()),
                    ("--dir", self.dir.is_some()),
                    ("--only", !self.only.is_empty()),
                    ("--update", self.update),
                    ("--plan", self.plan.is_some()),
                ];
                bad.extend(self.deck_strays());
                let bad = stray(&bad);
                ensure!(bad.is_empty(), "cardtest 不认识 {}（用 -h 看用法）", bad.join(" / "));
                if let Some(raw) = &self.metric {
                    Metric::parse(raw)?;
                }
                let correct = parse_correct(self.correct.as_deref())?;
                ensure!(!self.base.is_empty(), "cardtest 需要 --base <idrank,...>（基准卡组）");
                ensure!(!self.cards.is_empty(), "cardtest 需要 --cards <idrank,...>（候选列表）");
                ensure!(
                    !(self.greedy && self.slot.is_some()),
                    "--slot 与 --greedy 不能同时给：贪婪逐槽找最优，不看单一槽位"
                );
                if self.greedy_steps.is_some() {
                    ensure!(self.greedy, "--greedy-steps 需要 --greedy（单槽模式没有步数）");
                    let steps = self.greedy_steps.unwrap_or(0);
                    ensure!(steps >= 1, "--greedy-steps 必须 ≥ 1（收到 {steps}）");
                }
                if !self.greedy && self.slot.is_none() {
                    bail!("cardtest 需要 --slot <N>（变动槽位，0 基）或 --greedy");
                }
                Cmd::Cardtest(CardTestArgs {
                    base: self.base,
                    slot: self.slot,
                    cards: self.cards,
                    greedy: self.greedy,
                    greedy_steps: self.greedy_steps,
                    runs: self.runs,
                    seed: self.seed,
                    metric: self.metric,
                    goal: self.goal,
                    correct,
                    allow_cross_type: self.allow_cross_type,
                })
            }
            other => bail!("未知子命令 \"{other}\"（用 -h 看用法）"),
        };
        Ok(Args { cmd: Some(cmd), ..args })
    }

    /// 子命令专属选项是否出现过
    fn tail_given(&self) -> bool {
        self.runs.is_some()
            || self.seed.is_some()
            || self.search_n.is_some()
            || self.dir.is_some()
            || !self.only.is_empty()
            || self.update
            || self.plan.is_some()
            || self.deck_strays().iter().any(|(_, on)| *on)
            || self.cardtest_strays().iter().any(|(_, on)| *on)
            || self.tool_strays().iter().any(|(_, on)| *on)
    }

    /// `deck` 专属选项（供其余子命令点名，避免「某个子命令不认识的选项」被静默接受）
    fn deck_strays(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("--stage", self.stage.is_some()),
            ("--card-id-min", self.card_id_min.is_some()),
            ("--exclude", !self.exclude.is_empty()),
            ("--no-exclude", self.no_exclude),
            ("--seed-deck", !self.seed_deck.is_empty()),
            ("--top-k", self.top_k.is_some()),
            ("--slots", self.slots.is_some()),
            ("--fixed", !self.fixed.is_empty()),
            ("--prune", self.prune.is_some()),
            ("--screen-runs", self.screen_runs.is_some()),
            ("--fine-count", self.fine_count.is_some()),
            ("--holdout-seed", self.holdout_seed.is_some()),
            ("--holdout-count", self.holdout_count.is_some()),
            ("--max-per-type", self.max_per_type.is_some()),
            ("--min-types", self.min_types.is_some()),
        ]
    }

    /// `cardtest` 专属选项（供其余子命令点名）
    fn cardtest_strays(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("--base", !self.base.is_empty()),
            ("--slot", self.slot.is_some()),
            ("--cards", !self.cards.is_empty()),
            ("--allow-cross-type", self.allow_cross_type),
            ("--greedy", self.greedy),
            ("--greedy-steps", self.greedy_steps.is_some()),
        ]
    }

    /// 两个工具子命令共用的选项（其余子命令要点名）
    fn tool_strays(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("--metric", self.metric.is_some()),
            ("--goal", self.goal.is_some()),
            ("--correct", self.correct.is_some()),
        ]
    }
}

/// 子命令不认识的选项名（按声明顺序）
fn stray(pairs: &[(&'static str, bool)]) -> Vec<&'static str> {
    pairs.iter().filter(|(_, on)| *on).map(|(n, _)| *n).collect()
}

/// 解析多重比较校正口径（缺省不校正）
fn parse_correct(raw: Option<&str>) -> Result<Option<Correct>> {
    match raw {
        None => Ok(None),
        Some(text) => match Correct::parse(text) {
            Some(correct) => Ok(Some(correct)),
            None => bail!("--correct 只认 none / bonferroni / holm（收到 \"{text}\"）"),
        },
    }
}

/// 取一个开关的取值
fn text(parser: &mut lexopt::Parser, flag: &str) -> Result<String> {
    parser
        .value()
        .map_err(bad_arg)?
        .into_string()
        .map_err(|_| anyhow!("{flag} 的取值不是有效文本"))
}

/// 取一个开关的数值取值（解析失败即报错并指出是谁）
fn num<T: std::str::FromStr>(parser: &mut lexopt::Parser, flag: &str) -> Result<T> {
    let raw = text(parser, flag)?;
    raw.parse()
        .map_err(|_| anyhow!("{flag} 取值非法 \"{raw}\"（需要正整数）"))
}

/// 取一个开关的浮点取值（与 `num` 分开，错误文案才准确）
fn number(parser: &mut lexopt::Parser, flag: &str) -> Result<f64> {
    let raw = text(parser, flag)?;
    raw.parse()
        .map_err(|_| anyhow!("{flag} 取值非法 \"{raw}\"（需要数值，如 25）"))
}

/// 解析逗号分隔的非负整数列表（空段即报错，不静默跳过）
fn id_list(raw: &str, flag: &str) -> Result<Vec<u32>> {
    let mut out = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        ensure!(!part.is_empty(), "{flag} 含空段（形如 30257,30301）");
        out.push(
            part.parse()
                .map_err(|_| anyhow!("{flag} 取值非法 \"{part}\"（需要正整数，逗号分隔）"))?,
        );
    }
    Ok(out)
}

/// 解析「槽位 = idrank」（如 `5=303054`）
fn fixed_card(raw: &str, flag: &str) -> Result<FixedCard> {
    let (slot, id) = raw
        .split_once('=')
        .ok_or_else(|| anyhow!("{flag} 需要「槽位=idrank」形如 5=303054（收到 \"{raw}\"）"))?;
    let slot = slot
        .trim()
        .parse()
        .map_err(|_| anyhow!("{flag} 槽位非法 \"{slot}\"（需要 0 基下标）"))?;
    let id = id
        .trim()
        .parse()
        .map_err(|_| anyhow!("{flag} idrank 非法 \"{id}\"（需要正整数）"))?;
    Ok(FixedCard { slot, idrank: id })
}

/// 组合数 `C(n, k)`（`k > n` 记 0；`u128` 足够装下本卡池的枚举量）
fn choose(n: usize, k: usize) -> u128 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut acc: u128 = 1;
    for i in 0..k {
        acc = acc * (n - i) as u128 / (i + 1) as u128;
    }
    acc
}

/// 把解析错误统一成长句（带用法提示）
fn bad_arg(err: impl std::fmt::Display) -> anyhow::Error {
    anyhow!("{err}（用 -h 看用法）")
}

/// 入口：解析参数并执行子命令（`argv` 含程序名；输出写入 `out`）
///
/// 覆盖层目录取 `umaai-cli` 的约定值（当前目录）；要注入别的目录用
/// [`run_from_args_with_host_dir`]。
pub fn run_from_args(argv: &[String], out: &mut dyn Write) -> Result<()> {
    run_from_args_with_host_dir(argv, out, Path::new(umaai_cli::cli::CONFIG_DIR))
}

/// 入口（**宿主目录可注入**）：配置覆盖层取 `host_dir` 下的同名文件
///
/// 与 `umaai-cli` 同一条装配链；测试与内嵌宿主注入自己的目录，工具口径因此不随运行目录漂移。
pub fn run_from_args_with_host_dir(argv: &[String], out: &mut dyn Write, host_dir: &Path) -> Result<()> {
    let args = Args::parse(argv)?;
    let width = report::resolve_width(args.width);
    let Some(cmd) = args.cmd.clone() else {
        writeln!(out, "{}", Args::usage())?;
        return Ok(());
    };
    match cmd {
        Cmd::Bench { runs, seed } => run_bench(&args, width, runs, seed, host_dir, out),
        Cmd::Probe { seed, search_n } => run_probe(&args, width, seed, search_n, host_dir, out),
        Cmd::Baseline { dir, only, update } => run_baseline(&args, &dir, &only, update, host_dir, out),
        Cmd::Compare { plan } => run_compare(&args, width, &plan, host_dir, out),
        Cmd::Deck { plan, args: deck_args } => run_deck(&args, width, plan.as_deref(), &deck_args, host_dir, out),
        Cmd::Cardtest(test_args) => run_cardtest(&args, width, &test_args, host_dir, out),
    }
}

/// 载入配置（默认层 + 宿主目录覆盖层）并初始化静态数据
fn setup(data_root: &Path, host_dir: &Path) -> Result<Config> {
    // 与 `umaai-cli` 同一条链：配置先于静态数据，两份 ai_config.toml 都在数据加载之外
    let cfg = Config::load_with_overlay_file(data_root, host_dir)?;
    Data::init(data_root)?;
    Ok(cfg)
}

/// `bench`：批量跑局
fn run_bench(
    args: &Args,
    width: usize,
    runs: Option<u32>,
    seed: Option<u64>,
    host_dir: &Path,
    out: &mut dyn Write,
) -> Result<()> {
    let cfg = setup(&args.data_root, host_dir)?;
    let spec = batch::Spec {
        runs: runs.unwrap_or(cfg.sim.runs),
        seed: seed.unwrap_or(cfg.sim.seed),
    };
    let rep = batch::run(&cfg, &spec)?;

    if !args.quiet {
        writeln!(out, "{}", report::title("bench（固定种子批量跑局）", width))?;
        writeln!(
            out,
            "{}",
            report::key_values(&[
                ("数据根".to_string(), args.data_root.display().to_string()),
                ("局数".to_string(), spec.runs.to_string()),
                ("起始种子".to_string(), spec.seed.to_string()),
                ("评分均值".to_string(), fmt2(rep.dist.mean)),
                ("评分中位数".to_string(), fmt2(rep.dist.median)),
                (
                    "评分区间".to_string(),
                    format!("{} ~ {}", fmt2(rep.dist.min), fmt2(rep.dist.max))
                ),
                ("评分标准差".to_string(), fmt2(rep.dist.stdev)),
                ("PT 均值".to_string(), fmt2(rep.dist.pt_mean)),
                ("耗时".to_string(), format!("{} ms", rep.elapsed.as_millis())),
                ("吞吐".to_string(), format!("{} 局/秒", fmt2(rep.throughput()))),
            ])
        )?;
        let rows: Vec<Vec<String>> = rep
            .runs
            .iter()
            .map(|r| {
                vec![
                    r.index.to_string(),
                    r.seed.to_string(),
                    r.summary.score.to_string(),
                    r.summary.pt.to_string(),
                    r.summary.rank.clone(),
                    join(&r.summary.five_status),
                    format!("{} ms", r.elapsed.as_millis()),
                ]
            })
            .collect();
        writeln!(
            out,
            "{}",
            report::table_block(&["局", "种子", "评分", "PT", "档次", "五维", "耗时"], &rows, width)
        )?;
    }
    writeln!(
        out,
        "{}",
        report::machine_line(
            "BENCH",
            &[
                ("runs", spec.runs.to_string()),
                ("seed", spec.seed.to_string()),
                ("score_mean", fmt2(rep.dist.mean)),
                ("score_median", fmt2(rep.dist.median)),
                ("score_min", fmt2(rep.dist.min)),
                ("score_max", fmt2(rep.dist.max)),
                ("score_stdev", fmt2(rep.dist.stdev)),
                ("pt_mean", fmt2(rep.dist.pt_mean)),
                ("elapsed_ms", rep.elapsed.as_millis().to_string()),
                ("throughput", fmt2(rep.throughput())),
            ],
        )
    )?;
    out.flush()?;
    Ok(())
}

/// `probe`：固定根搜索成本探针
fn run_probe(
    args: &Args,
    width: usize,
    seed: Option<u64>,
    search_n: Option<usize>,
    host_dir: &Path,
    out: &mut dyn Write,
) -> Result<()> {
    let mut cfg = setup(&args.data_root, host_dir)?;
    if let Some(n) = search_n {
        if n == 0 {
            bail!("--search-n 必须 ≥ 1（零预算的搜索没有意义）");
        }
        cfg.search.search_n = n;
    }
    let seed = seed.unwrap_or(cfg.sim.seed);
    let rep = probe::search_root(&cfg, seed)?;

    if !args.quiet {
        writeln!(out, "{}", report::title("probe（固定根搜索成本）", width))?;
        writeln!(
            out,
            "{}",
            report::key_values(&[
                ("数据根".to_string(), args.data_root.display().to_string()),
                ("种子".to_string(), seed.to_string()),
                (
                    "根局面".to_string(),
                    format!("回合 {} / 阶段键 {}", rep.turn, rep.stage_key)
                ),
                ("候选数".to_string(), rep.candidates.to_string()),
                ("计划 rollout".to_string(), rep.total_planned().to_string()),
                (
                    "成功 / 失败".to_string(),
                    format!("{} / {}", rep.total_succeeded(), rep.total_failed())
                ),
                ("每候选预算".to_string(), cfg.search.search_n.to_string()),
                (
                    "中选动作".to_string(),
                    format!("[{}] {}", rep.best_idx, rep.best_action)
                ),
                (
                    "中选均分".to_string(),
                    fmt2(rep.means.get(rep.best_idx).copied().unwrap_or(0.0)),
                ),
                ("根局面评分".to_string(), fmt2(rep.root_score)),
                ("内核耗时".to_string(), format!("{} ms", rep.elapsed.as_millis())),
            ])
        )?;
        let rows: Vec<Vec<String>> = (0..rep.candidates)
            .map(|i| {
                vec![
                    i.to_string(),
                    rep.planned.get(i).copied().unwrap_or(0).to_string(),
                    rep.succeeded.get(i).copied().unwrap_or(0).to_string(),
                    rep.failed.get(i).copied().unwrap_or(0).to_string(),
                    fmt2(rep.means.get(i).copied().unwrap_or(0.0)),
                ]
            })
            .collect();
        writeln!(
            out,
            "{}",
            report::table_block(&["候选", "计划", "成功", "失败", "均分"], &rows, width)
        )?;
    }
    writeln!(
        out,
        "{}",
        report::machine_line(
            "PROBE",
            &[
                ("turn", rep.turn.to_string()),
                ("stage_key", rep.stage_key.to_string()),
                ("candidates", rep.candidates.to_string()),
                ("planned", rep.total_planned().to_string()),
                ("succeeded", rep.total_succeeded().to_string()),
                ("failed", rep.total_failed().to_string()),
                ("best_idx", rep.best_idx.to_string()),
                ("best_mean", fmt2(rep.means.get(rep.best_idx).copied().unwrap_or(0.0))),
                ("root_score", fmt2(rep.root_score)),
                ("elapsed_ms", rep.elapsed.as_millis().to_string()),
            ],
        )
    )?;
    out.flush()?;
    Ok(())
}

/// `compare`：分支推进对照（复盘用）
fn run_compare(args: &Args, width: usize, plan_path: &Path, host_dir: &Path, out: &mut dyn Write) -> Result<()> {
    let cfg = setup(&args.data_root, host_dir)?;
    let plan = compare::load_plan(plan_path)?;
    let rep = compare::run(&cfg, &plan)?;

    if !args.quiet {
        writeln!(out, "{}", report::title("compare（分支推进对照）", width))?;
        writeln!(
            out,
            "{}",
            report::key_values(&[
                ("对照计划".to_string(), plan_path.display().to_string()),
                ("规则主种子".to_string(), rep.seed.to_string()),
                ("探测回合".to_string(), rep.probe_turn.to_string()),
                (
                    "探测动作".to_string(),
                    format!("[{}] {}", plan.action, rep.probe_action)
                ),
                ("默认一选".to_string(), format!("[0] {}", rep.default_action)),
                ("探测回合数".to_string(), rep.planned_turns.to_string()),
                (
                    "实际观测".to_string(),
                    format!("默认 {} 回合 / 探测 {} 回合", rep.default_turns, rep.probe_turns),
                ),
            ])
        )?;
        let candidates: Vec<Vec<String>> = rep
            .candidates
            .iter()
            .enumerate()
            .map(|(i, name)| vec![i.to_string(), name.clone()])
            .collect();
        writeln!(
            out,
            "{}",
            report::table_block(&["下标", "探测回合候选"], &candidates, width)
        )?;
        let rows: Vec<Vec<String>> = rep
            .rows
            .iter()
            .map(|r| {
                vec![
                    r.turn.to_string(),
                    r.default.score.to_string(),
                    r.probe.score.to_string(),
                    r.score_delta().to_string(),
                    r.default.pt.to_string(),
                    r.probe.pt.to_string(),
                    r.pt_delta().to_string(),
                    r.five_delta().to_string(),
                ]
            })
            .collect();
        writeln!(
            out,
            "{}",
            report::table_block(
                &[
                    "回合",
                    "默认评分",
                    "探测评分",
                    "Δ评分",
                    "默认PT",
                    "探测PT",
                    "ΔPT",
                    "Δ五维"
                ],
                &rows,
                width
            )
        )?;
    }

    let mut fields: Vec<(&str, String)> = vec![
        ("seed", rep.seed.to_string()),
        ("probe_turn", rep.probe_turn.to_string()),
        ("turns", rep.planned_turns.to_string()),
        ("a_turns", rep.default_turns.to_string()),
        ("b_turns", rep.probe_turns.to_string()),
        ("a_action", report::token(&rep.default_action)),
        ("b_action", report::token(&rep.probe_action)),
    ];
    if let Some(row) = rep.last() {
        fields.push(("last_turn", row.turn.to_string()));
        fields.push(("a_score", row.default.score.to_string()));
        fields.push(("b_score", row.probe.score.to_string()));
        fields.push(("d_score", row.score_delta().to_string()));
        fields.push(("a_pt", row.default.pt.to_string()));
        fields.push(("b_pt", row.probe.pt.to_string()));
        fields.push(("d_pt", row.pt_delta().to_string()));
        fields.push(("a_five", compare::join_five(&row.default.five)));
        fields.push(("b_five", compare::join_five(&row.probe.five)));
    }
    writeln!(out, "{}", report::machine_line("COMPARE", &fields))?;
    out.flush()?;
    Ok(())
}

/// `deck`：组卡工具（阶段 `scan` / `compose` / `rank` / `all`）
///
/// 卡池 / 构成 / 参考卡组来自命令行与默认值（**实验口径自持**），配置链只提供局面装配参数
/// （马娘 / 继承 / 等级表 / 缺省种子）——与 `compare` 的计划自持同法。给了 `--plan` 时实验变量
/// 整份来自计划文件（入口已拒绝任何同名 CLI 开关），命令行只留公共项。
fn run_deck(
    args: &Args,
    width: usize,
    plan_path: Option<&Path>,
    opts: &DeckArgs,
    host_dir: &Path,
    out: &mut dyn Write,
) -> Result<()> {
    let cfg = setup(&args.data_root, host_dir)?;
    let from_plan;
    let opts = match plan_path {
        Some(path) => {
            from_plan = DeckArgs::from(&deck::load_plan(path)?);
            &from_plan
        }
        None => opts,
    };
    let spec = DeckSpec {
        card_id_min: opts.card_id_min.unwrap_or(DEFAULT_CARD_ID_MIN),
        exclude: opts.exclude.clone().unwrap_or_else(|| DEFAULT_EXCLUDE.to_vec()),
        rank: FULL_RANK,
        slots: opts.slots.unwrap_or(DEFAULT_TRAINING_SLOTS + opts.fixed.len()),
        fixed: opts.fixed.clone(),
        max_per_type: opts.max_per_type,
        min_types: opts.min_types,
    };
    spec.validate()?;
    let training = opts
        .seed_deck
        .clone()
        .unwrap_or_else(|| DEFAULT_REFERENCE_DECK.to_vec());
    match opts.stage {
        Stage::Scan => {
            let rep = deck_scan(&cfg, opts, &spec, &training)?;
            print_deck_scan(args, width, plan_path, opts, &spec, &rep, out)
        }
        Stage::Compose => run_deck_compose(args, width, plan_path, opts, &spec, out),
        Stage::Rank => {
            let rep = deck_rank(&cfg, opts, &spec, &training, None)?;
            print_deck_rank(args, width, plan_path, &spec, &rep, out)
        }
        Stage::All => {
            let scan = deck_scan(&cfg, opts, &spec, &training)?;
            print_deck_scan(args, width, plan_path, opts, &spec, &scan, out)?;
            let rep = deck_rank(&cfg, opts, &spec, &training, Some(&scan))?;
            print_deck_rank(args, width, plan_path, &spec, &rep, out)
        }
    }
}

/// 跑一次逐类型实测扫描（`scan` / `all` 共用）
fn deck_scan(cfg: &Config, opts: &DeckArgs, spec: &DeckSpec, training: &[u32]) -> Result<deck::ScanReport> {
    let runs = opts.runs.unwrap_or(DEFAULT_SCAN_RUNS);
    ensure!(runs >= 1, "--runs 必须 ≥ 1（收到 {runs}）");
    let seed = opts.seed.unwrap_or(cfg.sim.seed);
    let metric = Metric::parse(opts.metric.as_deref().unwrap_or("score"))?;
    deck::scan_decks(
        cfg,
        spec,
        training,
        &ScanSpec {
            runs,
            seed,
            metric,
            correct: opts.correct.unwrap_or(Correct::None),
        },
    )
}

/// 跑一次排序（`rank` / `all` 共用）
fn deck_rank(
    cfg: &Config,
    opts: &DeckArgs,
    spec: &DeckSpec,
    training: &[u32],
    scan: Option<&deck::ScanReport>,
) -> Result<deck::RankReport> {
    let runs = opts.runs.unwrap_or(DEFAULT_SCAN_RUNS);
    let metric = Metric::parse(opts.metric.as_deref().unwrap_or("score"))?;
    let mut rank_spec = deck::RankSpec {
        prune: opts.prune.unwrap_or(PruneMode::Measured),
        scan_runs: runs,
        runs,
        metric,
        seed: opts.seed.unwrap_or(cfg.sim.seed),
        ..deck::RankSpec::default()
    };
    if let Some(v) = opts.screen_runs {
        rank_spec.screen_runs = v;
    }
    if let Some(v) = opts.fine_count {
        rank_spec.fine_count = v;
    }
    if let Some(v) = opts.holdout_seed {
        rank_spec.holdout_seed = v;
    }
    if let Some(v) = opts.holdout_count {
        rank_spec.holdout_count = v;
    }
    if let Some(v) = opts.goal {
        rank_spec.goal = v;
    }
    if let Some(v) = opts.top_k {
        rank_spec.top_k = v;
    }
    if opts.holdout_seed.is_none() {
        // 缺省验证种子必须与选择集不同（规格自检会拒绝相同值）
        rank_spec.holdout_seed = rank_spec.seed.wrapping_add(1_000_000);
    }
    deck::rank(cfg, spec, training, &rank_spec, scan)
}

/// 呈现 `deck --stage scan`（计算在 [`deck_scan`]）
fn print_deck_scan(
    args: &Args,
    width: usize,
    plan_path: Option<&Path>,
    opts: &DeckArgs,
    spec: &DeckSpec,
    rep: &deck::ScanReport,
    out: &mut dyn Write,
) -> Result<()> {
    let metric = Metric::parse(opts.metric.as_deref().unwrap_or("score"))?;
    let correct = opts.correct.unwrap_or(Correct::None);
    let corrected = correct != Correct::None;
    if !args.quiet {
        writeln!(out, "{}", report::title("deck（逐类型实测扫描）", width))?;
        let mut head = vec![("数据根".to_string(), args.data_root.display().to_string())];
        if let Some(path) = plan_path {
            head.push(("实验计划".to_string(), path.display().to_string()));
        }
        head.push(("观测轴".to_string(), metric_name(metric).to_string()));
        if correct != Correct::None {
            head.push((
                "多重比较校正".to_string(),
                format!("{}（族 = 各类型榜，α = {}）", correct.name(), Correct::DEFAULT_ALPHA),
            ));
        }
        head.push((
            "卡池".to_string(),
            format!(
                "{} 张（cardId ≥ {}，排除 {} 张，突破档 {}）",
                rep.pool,
                spec.card_id_min,
                spec.exclude.len(),
                spec.rank
            ),
        ));
        head.push(("参考卡组".to_string(), join_u32(&rep.reference)));
        head.push((
            "未出榜类型".to_string(),
            if rep.skipped_types.is_empty() {
                "无".to_string()
            } else {
                format!(
                    "{}（参考卡组里没有该类型，需换参考卡组才会出榜）",
                    rep.skipped_types
                        .iter()
                        .map(|t| deck::type_name_zh(*t))
                        .collect::<Vec<_>>()
                        .join("/")
                )
            },
        ));
        head.push((
            "跳过重复候选".to_string(),
            format!("{} 张（与参考卡组其他槽位同卡，属重复卡组）", rep.skipped_duplicates),
        ));
        head.push(("每臂局数".to_string(), rep.runs.to_string()));
        head.push(("起始种子".to_string(), rep.seed.to_string()));
        head.push(("耗时".to_string(), format!("{} ms", rep.elapsed.as_millis())));
        writeln!(out, "{}", report::key_values(&head))?;
        writeln!(
            out,
            "口径：固定参考卡组其余槽位、只替换目标类型槽位，同种子配对；结论绑定参考卡组 / 训练员 / 局数，不外推为绝对强度。"
        )?;
        for board in &rep.boards {
            writeln!(
                out,
                "{}",
                report::title(
                    &format!(
                        "{} 类型（槽位 {}，参考卡名次 {}）",
                        deck::type_name_zh(board.card_type),
                        board.slot,
                        board.reference_rank
                    ),
                    width
                )
            )?;
            let mut header = vec!["名次", "卡ID", "卡名", "静态分", "Δ均值", "Δ标准差", "95%CI"];
            if corrected {
                header.extend_from_slice(&["校正CI", "校正p"]);
            }
            header.extend_from_slice(&["胜/负/平", "越过零", "备注"]);
            let rows: Vec<Vec<String>> = board
                .cards
                .iter()
                .enumerate()
                .map(|(idx, card)| {
                    let mut row = vec![
                        (idx + 1).to_string(),
                        card.card.card_id.to_string(),
                        card.card.name.clone(),
                        fmt2(card.card.panel.score),
                        fmt2(card.paired.mean),
                        fmt2(card.paired.sd),
                        fmt2(card.paired.ci95),
                    ];
                    if corrected {
                        row.push(fmt2(card.adjusted.ci));
                        row.push(fmt_p(card.adjusted.p_adj));
                    }
                    row.push(format!(
                        "{}/{}/{}",
                        card.paired.wins, card.paired.losses, card.paired.ties
                    ));
                    row.push(if card.adjusted.beyond {
                        "是".to_string()
                    } else {
                        "-".to_string()
                    });
                    row.push(if card.in_reference {
                        "参考".to_string()
                    } else {
                        String::new()
                    });
                    row
                })
                .collect();
            writeln!(out, "{}", report::table_block(&header, &rows, width))?;
        }
    }

    writeln!(
        out,
        "{}",
        report::machine_line(
            "DECK_SCAN",
            &[
                ("pool", rep.pool.to_string()),
                ("boards", rep.boards.len().to_string()),
                ("runs", rep.runs.to_string()),
                ("seed", rep.seed.to_string()),
                ("metric", metric_name(metric).to_string()),
                ("reference", join_u32(&rep.reference)),
                (
                    "skipped_types",
                    rep.skipped_types
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                ("skipped_duplicates", rep.skipped_duplicates.to_string()),
                ("correct", correct.name().to_string()),
                (
                    "plan",
                    plan_path.map_or("-".to_string(), |p| report::token(&p.display().to_string()))
                ),
                ("elapsed_ms", rep.elapsed.as_millis().to_string()),
            ],
        )
    )?;
    for board in &rep.boards {
        let Some(best) = board.cards.first() else {
            continue;
        };
        let mut fields = vec![
            ("type", board.card_type.to_string()),
            ("slot", board.slot.to_string()),
            ("best", best.card.idrank.to_string()),
            ("d_mean", fmt2(best.paired.mean)),
            ("d_ci95", fmt2(best.paired.ci95)),
        ];
        if corrected {
            fields.push(("d_ci_adj", fmt2(best.adjusted.ci)));
            fields.push(("p_adj", fmt_p(best.adjusted.p_adj)));
            fields.push(("beyond", u8::from(best.adjusted.beyond).to_string()));
        }
        fields.push(("ref_rank", board.reference_rank.to_string()));
        writeln!(out, "{}", report::machine_line("DECK_TYPE", &fields))?;
    }
    out.flush()?;
    Ok(())
}

/// `deck --stage compose`：构成枚举 + 静态代表卡
fn run_deck_compose(
    args: &Args,
    width: usize,
    plan_path: Option<&Path>,
    opts: &DeckArgs,
    spec: &DeckSpec,
    out: &mut dyn Write,
) -> Result<()> {
    let reps = deck::pool(spec)?;
    let comps = deck::compositions(spec)?;
    let fixed_types = deck::fixed_card_types(spec)?;
    let top_k = opts.top_k.unwrap_or(DEFAULT_TOP_K);

    // pool 已按「类型升序 → 面板分降序」排好，每类型截断到 Top-K 即静态代表卡
    let mut by_type: [Vec<&deck::CardRep>; 5] = std::array::from_fn(|_| Vec::new());
    let mut pool_counts = [0usize; 5];
    for rep in &reps {
        if (0..5).contains(&rep.card_type) {
            pool_counts[rep.card_type as usize] += 1;
            let bucket = &mut by_type[rep.card_type as usize];
            if bucket.len() < top_k {
                bucket.push(rep);
            }
        }
    }
    let capped: [usize; 5] = std::array::from_fn(|i| by_type[i].len());
    let decks_all: u128 = comps.iter().map(|c| comp_decks(c, &pool_counts)).sum();
    let decks_topk: u128 = comps.iter().map(|c| comp_decks(c, &capped)).sum();

    if !args.quiet {
        writeln!(out, "{}", report::title("deck（构成枚举 + 静态代表卡）", width))?;
        let mut head = vec![("数据根".to_string(), args.data_root.display().to_string())];
        if let Some(path) = plan_path {
            head.push(("实验计划".to_string(), path.display().to_string()));
        }
        head.push((
            "卡池".to_string(),
            format!(
                "{} 张（cardId ≥ {}，排除 {} 张，突破档 {}）",
                reps.len(),
                spec.card_id_min,
                spec.exclude.len(),
                spec.rank
            ),
        ));
        head.push((
            "槽位".to_string(),
            format!(
                "{}（训练卡 {}，必带 {}）",
                spec.slots,
                spec.training_slots(),
                if spec.fixed.is_empty() {
                    "无".to_string()
                } else {
                    spec.fixed
                        .iter()
                        .map(|f| format!("{}={}", f.slot, f.idrank))
                        .collect::<Vec<_>>()
                        .join(",")
                }
            ),
        ));
        head.push((
            "构成约束".to_string(),
            format!(
                "单类型上限 {} / 卡种下限 {}（全卡组 7 类，必带卡计入）",
                spec.max_per_type.map_or("不限".to_string(), |v| v.to_string()),
                spec.min_types.map_or("不限".to_string(), |v| v.to_string())
            ),
        ));
        head.push(("构成数".to_string(), comps.len().to_string()));
        head.push(("卡组数（全池）".to_string(), decks_all.to_string()));
        head.push((
            "卡组数（Top-K）".to_string(),
            format!("{decks_topk}（每类型 {top_k} 张代表卡）"),
        ));
        writeln!(out, "{}", report::key_values(&head))?;
        writeln!(
            out,
            "说明：静态面板分 = 友情×2 + 干劲×0.5 + 训练，**只用于剪枝与展示，不是强度排名**；精确排序走 --stage scan。"
        )?;

        writeln!(out, "{}", report::title("静态代表卡", width))?;
        let mut rows: Vec<Vec<String>> = Vec::new();
        for (idx, bucket) in by_type.iter().enumerate() {
            for (rank, rep) in bucket.iter().enumerate() {
                rows.push(vec![
                    deck::type_name_zh(idx as i32).to_string(),
                    (rank + 1).to_string(),
                    rep.card_id.to_string(),
                    rep.name.clone(),
                    fmt2(rep.panel.score),
                    fmt2(f64::from(rep.panel.youqing)),
                    rep.panel.xunlian.to_string(),
                    fmt2(f64::from(rep.panel.deyilv)),
                    rep.panel.jiban.to_string(),
                    rep.panel.saihou.to_string(),
                ]);
            }
        }
        writeln!(
            out,
            "{}",
            report::table_block(
                &[
                    "类型",
                    "名次",
                    "卡ID",
                    "卡名",
                    "静态分",
                    "友情",
                    "训练",
                    "得意率",
                    "羁绊",
                    "赛后"
                ],
                &rows,
                width
            )
        )?;

        writeln!(out, "{}", report::title("构成清单", width))?;
        let rows: Vec<Vec<String>> = comps
            .iter()
            .map(|comp| {
                vec![
                    comp.label(),
                    comp.counts
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("/"),
                    comp.kinds_with(&fixed_types).to_string(),
                    comp_decks(comp, &pool_counts).to_string(),
                    comp_decks(comp, &capped).to_string(),
                ]
            })
            .collect();
        writeln!(
            out,
            "{}",
            report::table_block(
                &[
                    "构成",
                    "速/耐/力/根/智",
                    "卡种数（含必带）",
                    "卡组数（全池）",
                    "卡组数（Top-K）"
                ],
                &rows,
                width
            )
        )?;
    }

    writeln!(
        out,
        "{}",
        report::machine_line(
            "DECK_COMPOSE",
            &[
                ("pool", reps.len().to_string()),
                ("slots", spec.slots.to_string()),
                ("training_slots", spec.training_slots().to_string()),
                ("compositions", comps.len().to_string()),
                ("decks_all", decks_all.to_string()),
                ("decks_topk", decks_topk.to_string()),
                ("top_k", top_k.to_string()),
                (
                    "plan",
                    plan_path.map_or("-".to_string(), |p| report::token(&p.display().to_string()))
                ),
            ],
        )
    )?;
    out.flush()?;
    Ok(())
}

/// 构成在给定池规模下的卡组数（各类型独立取卡，故为组合数之积）
fn comp_decks(comp: &deck::Composition, counts: &[usize; 5]) -> u128 {
    (0..5).map(|i| choose(counts[i], comp.counts[i])).product()
}

/// 观测轴名（机读行与呈现共用）
fn metric_name(metric: Metric) -> &'static str {
    match metric {
        Metric::Score => "score",
        Metric::Pt => "pt",
        Metric::FiveSum => "five",
    }
}

/// 呈现 `deck --stage rank`（计算在 [`deck_rank`]）
fn print_deck_rank(
    args: &Args,
    width: usize,
    plan_path: Option<&Path>,
    spec: &DeckSpec,
    rep: &deck::RankReport,
    out: &mut dyn Write,
) -> Result<()> {
    if !args.quiet {
        writeln!(
            out,
            "{}",
            report::title("deck（排序：粗筛 → 精评 → 独立种子集验证）", width)
        )?;
        let mut head = vec![("数据根".to_string(), args.data_root.display().to_string())];
        if let Some(path) = plan_path {
            head.push(("实验计划".to_string(), path.display().to_string()));
        }
        head.push((
            "卡池".to_string(),
            format!(
                "{} 张（cardId ≥ {}，排除 {} 张，突破档 {}）",
                rep.pool,
                spec.card_id_min,
                spec.exclude.len(),
                spec.rank
            ),
        ));
        head.push(("参考卡组".to_string(), join_u32(&rep.reference)));
        head.push((
            "剪枝口径".to_string(),
            format!("{}（每类型前 {} 张代表卡）", rep.prune.name(), rep.spec.top_k),
        ));
        head.push((
            "候选卡组".to_string(),
            format!(
                "{} 副（一种构成一副；跳过 {} 种装配不出的构成）",
                rep.candidates, rep.skipped_unfillable
            ),
        ));
        head.push((
            "回退静态序类型".to_string(),
            if rep.fallback_types.is_empty() {
                "无".to_string()
            } else {
                format!(
                    "{}（参考卡组里没有该类型，无实测榜，按静态面板序兜底）",
                    rep.fallback_types
                        .iter()
                        .map(|t| deck::type_name_zh(*t))
                        .collect::<Vec<_>>()
                        .join("/")
                )
            },
        ));
        head.push((
            "粗筛".to_string(),
            format!("{} 局/臂 × {} 副", rep.spec.screen_runs, rep.candidates),
        ));
        head.push((
            "精评".to_string(),
            format!(
                "{} 局/臂 × {} 副",
                rep.spec.runs,
                rep.spec.fine_count.min(rep.candidates)
            ),
        ));
        head.push((
            "验证".to_string(),
            format!(
                "{} 局/臂 × {} 副（种子 {}，只复验不参与排序）",
                rep.spec.holdout_runs,
                rep.spec.holdout_count.min(rep.candidates),
                rep.spec.holdout_seed
            ),
        ));
        head.push(("耗时".to_string(), format!("{} ms", rep.elapsed.as_millis())));
        writeln!(out, "{}", report::key_values(&head))?;
        writeln!(
            out,
            "口径：候选 = 一种构成一副代表卡组（沿用 bench_compositions 口径，不做全空间穷举）；三级测量都走同种子配对；结论绑定训练员与局数。"
        )?;
        let rows: Vec<Vec<String>> = rep
            .rows
            .iter()
            .enumerate()
            .map(|(idx, row)| {
                vec![
                    (idx + 1).to_string(),
                    row.composition.label(),
                    join_u32(&row.deck),
                    fmt2(row.panel_score),
                    fmt2(row.screen.paired.mean),
                    row.fine.as_ref().map_or("-".to_string(), |m| fmt2(m.paired.mean)),
                    row.fine.as_ref().map_or("-".to_string(), |m| fmt2(m.paired.ci95)),
                    row.holdout.as_ref().map_or("-".to_string(), |m| fmt2(m.paired.mean)),
                    row.fine
                        .as_ref()
                        .map_or(row.screen.runs_for_goal, |m| m.runs_for_goal)
                        .to_string(),
                    row.tier_name().to_string(),
                ]
            })
            .collect();
        writeln!(
            out,
            "{}",
            report::table_block(
                &[
                    "名次",
                    "构成",
                    "卡组",
                    "静态分",
                    "粗筛Δ",
                    "精评Δ",
                    "精评CI",
                    "验证Δ",
                    "建议局数",
                    "层级"
                ],
                &rows,
                width
            )
        )?;
    }

    writeln!(
        out,
        "{}",
        report::machine_line(
            "DECK_RANK",
            &[
                ("pool", rep.pool.to_string()),
                ("candidates", rep.candidates.to_string()),
                ("skipped", rep.skipped_unfillable.to_string()),
                ("prune", rep.prune.name().to_string()),
                (
                    "fallback",
                    rep.fallback_types
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                ("top_k", rep.spec.top_k.to_string()),
                ("screen_runs", rep.spec.screen_runs.to_string()),
                ("runs", rep.spec.runs.to_string()),
                ("holdout_runs", rep.spec.holdout_runs.to_string()),
                ("seed", rep.spec.seed.to_string()),
                ("holdout_seed", rep.spec.holdout_seed.to_string()),
                (
                    "plan",
                    plan_path.map_or("-".to_string(), |p| report::token(&p.display().to_string()))
                ),
                ("elapsed_ms", rep.elapsed.as_millis().to_string()),
            ],
        )
    )?;
    for (idx, row) in rep.rows.iter().take(RANK_TOP_LINES).enumerate() {
        writeln!(
            out,
            "{}",
            report::machine_line(
                "DECK_TOP",
                &[
                    ("rank", (idx + 1).to_string()),
                    ("comp", row.composition.label()),
                    ("deck", join_u32(&row.deck)),
                    ("d_screen", fmt2(row.screen.paired.mean)),
                    ("d_fine", row.fine.as_ref().map_or("-".into(), |m| fmt2(m.paired.mean))),
                    (
                        "d_holdout",
                        row.holdout.as_ref().map_or("-".into(), |m| fmt2(m.paired.mean))
                    ),
                    ("tier", row.tier_name().to_string()),
                ],
            )
        )?;
    }
    out.flush()?;
    Ok(())
}

/// `cardtest`：固定基准卡组、只换一个槽位（或逐槽贪婪），逐候选同种子配对
///
/// 基准卡组与候选列表**全部由命令行给**（实验口径自持），配置链只提供局面装配参数；
/// 剧本仓换成自己的装配 + 手写训练员时，用 [`crate::cardtest::run`] / [`crate::cardtest::run_greedy`]
/// 同一套守卫与统计口径。
fn run_cardtest(args: &Args, width: usize, opts: &CardTestArgs, host_dir: &Path, out: &mut dyn Write) -> Result<()> {
    let cfg = setup(&args.data_root, host_dir)?;
    let metric = Metric::parse(opts.metric.as_deref().unwrap_or("score"))?;
    let runs = opts.runs.unwrap_or(DEFAULT_SCAN_RUNS);
    let seed = opts.seed.unwrap_or(cfg.sim.seed);
    let goal = opts.goal.unwrap_or(crate::arms::DEFAULT_GOAL);
    let correct = opts.correct.unwrap_or(Correct::None);

    if opts.greedy {
        let spec = cardtest::GreedySpec {
            candidates: opts.cards.clone(),
            runs,
            seed,
            metric,
            goal,
            allow_cross_type: opts.allow_cross_type,
            max_steps: opts.greedy_steps.unwrap_or(DEFAULT_GREEDY_STEPS),
            correct,
        };
        let rep = cardtest::run_greedy(&cfg, &opts.base, &spec)?;
        return print_cardtest_greedy(args, width, &rep, out);
    }

    let spec = cardtest::Spec {
        slot: opts
            .slot
            .ok_or_else(|| anyhow!("单槽模式需要 --slot <N>（0 基变动槽位）"))?,
        candidates: opts.cards.clone(),
        runs,
        seed,
        metric,
        goal,
        allow_cross_type: opts.allow_cross_type,
        correct,
    };
    let rep = cardtest::run(&cfg, &opts.base, &spec)?;
    let corrected = correct != Correct::None;

    if !args.quiet {
        writeln!(
            out,
            "{}",
            report::title("cardtest（固定基准卡组、只换一个槽位）", width)
        )?;
        let mut head = vec![
            ("数据根".to_string(), args.data_root.display().to_string()),
            ("基准卡组".to_string(), join_u32(&rep.base)),
            (
                "变动槽位".to_string(),
                format!(
                    "{}（{}类型，原卡 {}）",
                    rep.slot,
                    deck::type_name_zh(rep.base_type),
                    rep.base.get(rep.slot).copied().unwrap_or(0)
                ),
            ),
            ("候选数".to_string(), rep.rows.len().to_string()),
            ("每臂局数".to_string(), rep.spec.runs.to_string()),
            ("起始种子".to_string(), rep.spec.seed.to_string()),
            ("观测轴".to_string(), metric_name(rep.spec.metric).to_string()),
            (
                "目标分辨率".to_string(),
                format!("{}（用于反推所需局数）", fmt2(rep.spec.goal)),
            ),
        ];
        if corrected {
            head.push((
                "多重比较校正".to_string(),
                format!("{}（族 = 本批候选，α = {}）", correct.name(), Correct::DEFAULT_ALPHA),
            ));
        }
        head.push(("耗时".to_string(), format!("{} ms", rep.elapsed.as_millis())));
        writeln!(out, "{}", report::key_values(&head))?;
        writeln!(
            out,
            "口径：其余槽位逐位不变、只替换变动槽位，同种子配对；结论只在该基准卡组与训练员下成立。"
        )?;
        let mut header = vec!["名次", "卡ID", "卡名", "均值", "Δ均值", "Δ标准差", "95%CI"];
        if corrected {
            header.extend_from_slice(&["校正CI", "校正p"]);
        }
        header.extend_from_slice(&["胜/负/平", "建议局数", "越过零", "备注"]);
        let rows: Vec<Vec<String>> = rep
            .rows
            .iter()
            .enumerate()
            .map(|(idx, row)| {
                let mut cells = vec![
                    (idx + 1).to_string(),
                    row.card_id.to_string(),
                    row.name.clone(),
                    fmt2(row.measured.mean),
                    fmt2(row.measured.paired.mean),
                    fmt2(row.measured.paired.sd),
                    fmt2(row.measured.paired.ci95),
                ];
                if corrected {
                    cells.push(fmt2(row.adjusted.ci));
                    cells.push(fmt_p(row.adjusted.p_adj));
                }
                cells.push(format!(
                    "{}/{}/{}",
                    row.measured.paired.wins, row.measured.paired.losses, row.measured.paired.ties
                ));
                cells.push(row.measured.runs_for_goal.to_string());
                cells.push(if row.adjusted.beyond {
                    "是".to_string()
                } else {
                    "-".to_string()
                });
                cells.push(if row.is_reference {
                    "基准".to_string()
                } else {
                    String::new()
                });
                cells
            })
            .collect();
        writeln!(out, "{}", report::table_block(&header, &rows, width))?;
    }

    let mut fields = vec![
        ("slot", rep.slot.to_string()),
        ("base_type", rep.base_type.to_string()),
        ("base", join_u32(&rep.base)),
        ("candidates", rep.rows.len().to_string()),
        ("runs", rep.spec.runs.to_string()),
        ("seed", rep.spec.seed.to_string()),
        ("metric", metric_name(rep.spec.metric).to_string()),
        ("goal", fmt2(rep.spec.goal)),
        ("correct", correct.name().to_string()),
        ("elapsed_ms", rep.elapsed.as_millis().to_string()),
    ];
    writeln!(out, "{}", report::machine_line("CARDTEST", &fields))?;
    for (idx, row) in rep.rows.iter().enumerate() {
        fields = vec![
            ("rank", (idx + 1).to_string()),
            ("card", row.idrank.to_string()),
            ("name", report::token(&row.name)),
            ("d_mean", fmt2(row.measured.paired.mean)),
            ("d_sd", fmt2(row.measured.paired.sd)),
            ("d_ci95", fmt2(row.measured.paired.ci95)),
            ("wins", row.measured.paired.wins.to_string()),
            ("losses", row.measured.paired.losses.to_string()),
            ("need", row.measured.runs_for_goal.to_string()),
        ];
        if corrected {
            fields.push(("d_ci_adj", fmt2(row.adjusted.ci)));
            fields.push(("p_adj", fmt_p(row.adjusted.p_adj)));
            fields.push(("beyond", u8::from(row.adjusted.beyond).to_string()));
        }
        fields.push(("reference", u8::from(row.is_reference).to_string()));
        writeln!(out, "{}", report::machine_line("CARDTEST_ROW", &fields))?;
    }
    out.flush()?;
    Ok(())
}

/// 呈现逐槽贪婪结果（计算在 [`cardtest::run_greedy`]）
///
/// 人读：逐步给出该步的全部「槽位 × 候选」排行与采纳结果；机读：一行一步（`CARDTEST_STEP`），
/// 并把**最后一步**的全部组合打成 `CARDTEST_PAIR`——那是「为什么停下 / 为什么继续」的凭据。
fn print_cardtest_greedy(args: &Args, width: usize, rep: &cardtest::GreedyReport, out: &mut dyn Write) -> Result<()> {
    let corrected = rep.spec.correct != Correct::None;
    let pairs_total: usize = rep.steps.iter().map(|step| step.rows.len()).sum();
    if !args.quiet {
        writeln!(
            out,
            "{}",
            report::title(
                "cardtest（逐槽贪婪：每步测「槽位 × 候选」，取最优且越过零者替换）",
                width
            )
        )?;
        let mut head = vec![
            ("数据根".to_string(), args.data_root.display().to_string()),
            ("基准卡组".to_string(), join_u32(&rep.base)),
            ("候选数".to_string(), rep.spec.candidates.len().to_string()),
            ("每臂局数".to_string(), rep.spec.runs.to_string()),
            ("起始种子".to_string(), rep.spec.seed.to_string()),
            ("观测轴".to_string(), metric_name(rep.spec.metric).to_string()),
            (
                "目标分辨率".to_string(),
                format!("{}（用于反推所需局数）", fmt2(rep.spec.goal)),
            ),
            ("步数上限".to_string(), rep.spec.max_steps.to_string()),
            (
                "停止原因".to_string(),
                match rep.stop {
                    cardtest::Stop::NoImprovement => "无越过零的正向改进".to_string(),
                    cardtest::Stop::StepsExhausted => "用满步数上限（如需继续请加大 --greedy-steps）".to_string(),
                },
            ),
            ("最终卡组".to_string(), join_u32(&rep.deck)),
        ];
        if corrected {
            head.push((
                "多重比较校正".to_string(),
                format!(
                    "{}（族 = 每步的组合，α = {}）",
                    rep.spec.correct.name(),
                    Correct::DEFAULT_ALPHA
                ),
            ));
        }
        head.push(("耗时".to_string(), format!("{} ms", rep.elapsed.as_millis())));
        writeln!(out, "{}", report::key_values(&head))?;
        writeln!(
            out,
            "口径：每步的配对都相对**该步**的卡组（步与步之间卡组不同，Δ 不可跨步比较）；\
             采纳判据是「Δ 均值 > 0 且校正后越过零」，无改进即停。"
        )?;

        for step in &rep.steps {
            writeln!(
                out,
                "{}",
                report::title(
                    &format!("第 {} 步（替换前卡组 {}）", step.index + 1, join_u32(&step.deck_before)),
                    width
                )
            )?;
            let mut header = vec!["名次", "槽位", "类型", "卡ID", "卡名", "Δ均值", "Δ标准差", "95%CI"];
            if corrected {
                header.extend_from_slice(&["校正CI", "校正p"]);
            }
            header.extend_from_slice(&["建议局数", "越过零"]);
            let rows: Vec<Vec<String>> = step
                .rows
                .iter()
                .enumerate()
                .map(|(idx, row)| {
                    let mut cells = vec![
                        (idx + 1).to_string(),
                        row.slot.to_string(),
                        deck::type_name_zh(row.card_type).to_string(),
                        row.card_id.to_string(),
                        row.name.clone(),
                        fmt2(row.measured.paired.mean),
                        fmt2(row.measured.paired.sd),
                        fmt2(row.measured.paired.ci95),
                    ];
                    if corrected {
                        cells.push(fmt2(row.adjusted.ci));
                        cells.push(fmt_p(row.adjusted.p_adj));
                    }
                    cells.push(row.measured.runs_for_goal.to_string());
                    cells.push(if row.adjusted.beyond {
                        "是".to_string()
                    } else {
                        "-".to_string()
                    });
                    cells
                })
                .collect();
            writeln!(out, "{}", report::table_block(&header, &rows, width))?;
            match &step.chosen {
                Some(best) => writeln!(
                    out,
                    "采纳：槽位 {} ← {}（{}，Δ均值 {}，卡组 → {}）",
                    best.slot,
                    best.idrank,
                    best.name,
                    fmt2(best.measured.paired.mean),
                    join_u32(&rep.deck_after_step(step.index))
                )?,
                None => writeln!(out, "停止：本步没有越过零的正向改进（卡组不变）")?,
            }
        }
    }

    writeln!(
        out,
        "{}",
        report::machine_line(
            "CARDTEST_GREEDY",
            &[
                ("base", join_u32(&rep.base)),
                ("final", join_u32(&rep.deck)),
                ("candidates", rep.spec.candidates.len().to_string()),
                ("steps", rep.steps.len().to_string()),
                ("pairs", pairs_total.to_string()),
                ("stop", rep.stop.name().to_string()),
                ("runs", rep.spec.runs.to_string()),
                ("seed", rep.spec.seed.to_string()),
                ("metric", metric_name(rep.spec.metric).to_string()),
                ("goal", fmt2(rep.spec.goal)),
                ("correct", rep.spec.correct.name().to_string()),
                ("elapsed_ms", rep.elapsed.as_millis().to_string()),
            ],
        )
    )?;
    for step in &rep.steps {
        let mut fields = vec![("step", step.index.to_string())];
        match &step.chosen {
            Some(best) => {
                fields.push(("slot", best.slot.to_string()));
                fields.push(("card", best.idrank.to_string()));
                fields.push(("d_mean", fmt2(best.measured.paired.mean)));
                fields.push(("d_ci95", fmt2(best.measured.paired.ci95)));
                if corrected {
                    fields.push(("d_ci_adj", fmt2(best.adjusted.ci)));
                    fields.push(("p_adj", fmt_p(best.adjusted.p_adj)));
                }
            }
            None => {
                fields.push(("slot", "-".to_string()));
                fields.push(("card", "-".to_string()));
                fields.push(("d_mean", "-".to_string()));
                fields.push(("d_ci95", "-".to_string()));
            }
        }
        fields.push(("chosen", u8::from(step.chosen.is_some()).to_string()));
        fields.push(("deck", join_u32(&rep.deck_after_step(step.index))));
        writeln!(out, "{}", report::machine_line("CARDTEST_STEP", &fields))?;
    }
    if let Some(last) = rep.steps.last() {
        for row in &last.rows {
            let mut fields = vec![
                ("step", last.index.to_string()),
                ("slot", row.slot.to_string()),
                ("card", row.idrank.to_string()),
                ("name", report::token(&row.name)),
                ("d_mean", fmt2(row.measured.paired.mean)),
                ("d_ci95", fmt2(row.measured.paired.ci95)),
            ];
            if corrected {
                fields.push(("d_ci_adj", fmt2(row.adjusted.ci)));
                fields.push(("p_adj", fmt_p(row.adjusted.p_adj)));
            }
            fields.push(("beyond", u8::from(row.adjusted.beyond).to_string()));
            writeln!(out, "{}", report::machine_line("CARDTEST_PAIR", &fields))?;
        }
    }
    out.flush()?;
    Ok(())
}

/// `baseline`：校验或重录
fn run_baseline(
    args: &Args,
    dir: &Path,
    only: &[String],
    update: bool,
    host_dir: &Path,
    out: &mut dyn Write,
) -> Result<()> {
    setup(&args.data_root, host_dir)?;
    let files = baseline::list_cases(dir)?;
    let selected: Vec<PathBuf> = files
        .into_iter()
        .filter(|path| only.is_empty() || path.file_stem().is_some_and(|s| only.iter().any(|o| s == o.as_str())))
        .collect();
    if selected.is_empty() {
        bail!("--only 没有匹配到任何基线（可用: {}）", only.join(" / "));
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut updated = 0usize;
    let mut details: Vec<String> = Vec::new();
    for path in &selected {
        let case = baseline::load_case(path)?;
        if update {
            baseline::save_case(path, &baseline::record(&case)?)?;
            updated += 1;
            if !args.quiet {
                writeln!(out, "重录 {} ({})", case.case, path.display())?;
            }
            continue;
        }
        let diffs = check(&case)?;
        if diffs.is_empty() {
            passed += 1;
            if !args.quiet {
                writeln!(out, "ok   {} ({})", case.case, expect_summary(&case))?;
            }
        } else {
            failed += 1;
            for d in &diffs {
                details.push(format!("{} {}: {}", case.case, d.key, d.detail));
            }
            if !args.quiet {
                writeln!(out, "FAIL {}（{} 处不一致）", case.case, diffs.len())?;
            }
        }
    }
    for line in &details {
        writeln!(out, "  {line}")?;
    }
    writeln!(
        out,
        "{}",
        report::machine_line(
            "BASELINE",
            &[
                ("dir", report::token(&dir.display().to_string())),
                ("files", selected.len().to_string()),
                ("passed", passed.to_string()),
                ("failed", failed.to_string()),
                ("updated", updated.to_string()),
            ],
        )
    )?;
    out.flush()?;
    if failed > 0 {
        bail!("{failed} 份基线与实测不一致（重录: umaai-tools baseline --update）");
    }
    Ok(())
}

/// 跑一份基线并与期望比对
fn check(case: &Case) -> Result<Vec<Difference>> {
    let facts = baseline::run_case(case)?;
    Ok(baseline::compare(&facts, &case.expect))
}

/// 基线摘要（人读用：键数与记录日期）
fn expect_summary(case: &Case) -> String {
    format!("{} 项观测，记录于 {}", case.expect.len(), case.meta.updated)
}

/// 保留两位小数的数值文本
fn fmt2(v: f64) -> String {
    format!("{v:.2}")
}

/// p 值与校正 p 的呈现文本
///
/// 两位小数会把整段小 p 压成 `0.00`（多重比较里最常见的那一档），故小值切科学计数；
/// 正态近似本身的误差量级约 1e-7，比它更小的 p 只报 `<1e-7`（不假装有那个分辨率）；
/// 恰好 1 的封顶值写 `1`，非有限值给 `-`。
fn fmt_p(v: f64) -> String {
    if !v.is_finite() {
        return "-".to_string();
    }
    if v < 1e-7 {
        "<1e-7".to_string()
    } else if v < 1e-4 {
        format!("{v:.1e}")
    } else if v >= 1.0 {
        "1".to_string()
    } else {
        format!("{v:.4}")
    }
}

/// 整数数组转逗号分隔（机读行与表格共用；不使用 `{:?}`，避免空格破坏切分）
fn join(v: &[i32]) -> String {
    v.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")
}

/// `u32` 数组转逗号分隔
fn join_u32(v: &[u32]) -> String {
    v.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造 argv（含程序名）
    fn argv(rest: &[&str]) -> Vec<String> {
        std::iter::once("umaai-tools".to_string())
            .chain(rest.iter().map(|s| (*s).to_string()))
            .collect()
    }

    /// 层1：无参数只打用法，不选子命令
    #[test]
    fn test_parse_empty() {
        let args = Args::parse(&argv(&[])).expect("空参数应可解析");
        assert!(args.cmd.is_none());
        assert_eq!(args.data_root, PathBuf::from(DEFAULT_DATA_ROOT));
        assert!(!args.quiet);
    }

    /// 层1：逐项取值
    #[test]
    fn test_parse_values() -> Result<()> {
        let args = Args::parse(&argv(&[
            "bench",
            "--runs",
            "8",
            "--seed",
            "7",
            "--data-root",
            "/tmp/d",
            "--width",
            "88",
            "--quiet",
        ]))?;
        assert_eq!(args.cmd, Some(Cmd::Bench { runs: Some(8), seed: Some(7) }));
        assert_eq!(args.data_root, PathBuf::from("/tmp/d"));
        assert_eq!(args.width, Some(88));
        assert!(args.quiet);

        let args = Args::parse(&argv(&["probe", "--search-n", "16"]))?;
        assert_eq!(args.cmd, Some(Cmd::Probe { seed: None, search_n: Some(16) }));
        assert_eq!(
            args.data_root,
            PathBuf::from(DEFAULT_DATA_ROOT),
            "公共选项缺省值不受子命令影响"
        );

        let args = Args::parse(&argv(&["baseline", "--only", "a", "--only", "b", "--update"]))?;
        assert_eq!(
            args.cmd,
            Some(Cmd::Baseline {
                dir: PathBuf::from(DEFAULT_BASELINE_DIR),
                only: vec!["a".to_string(), "b".to_string()],
                update: true,
            })
        );

        let args = Args::parse(&argv(&["compare", "--plan", "plan.json"]))?;
        assert_eq!(
            args.cmd,
            Some(Cmd::Compare {
                plan: PathBuf::from("plan.json"),
            })
        );
        Ok(())
    }

    /// 层1：选项与子命令的先后无关，`--opt=value` 与短选项写法同样生效
    #[test]
    fn test_parse_order_and_forms() -> Result<()> {
        let want = Some(Cmd::Bench { runs: Some(8), seed: Some(7) });
        for case in [
            vec!["bench", "--runs", "8", "--seed", "7"],
            vec!["--runs", "8", "bench", "--seed", "7"],
            vec!["--data-root", "/tmp/d", "bench", "--runs=8", "--seed=7"],
        ] {
            let args = Args::parse(&argv(&case))?;
            assert_eq!(args.cmd, want, "{case:?} 应解析出同一结果");
        }

        let args = Args::parse(&argv(&["--data-root=/tmp/d", "--quiet", "probe"]))?;
        assert_eq!(args.data_root, PathBuf::from("/tmp/d"), "--opt=value 形式应生效");
        assert!(args.quiet && args.cmd == Some(Cmd::Probe { seed: None, search_n: None }));
        Ok(())
    }

    /// 层1：未知子命令、陌生开关、缺值、非法数值、过窄宽度、选项归属错都要报错
    #[test]
    fn test_parse_errors() {
        for bad in [
            vec!["nope"],
            vec!["bench", "--nope"],
            vec!["bench", "extra"],
            vec!["probe", "--runs", "3"],
            vec!["probe", "--dir", "x"],
            vec!["baseline", "--runs", "1"],
            vec!["baseline", "--seed", "1"],
            vec!["baseline", "--update", "--nope"],
            vec!["bench", "--plan", "x"],
            vec!["compare", "--runs", "1"],
            vec!["compare"],
            vec!["--quiet"],
            vec!["--data-root", "/tmp/d"],
            vec!["bench", "--runs"],
            vec!["bench", "--runs", "abc"],
            vec!["--width", "39"],
            vec!["--width", "0"],
            vec!["deck", "--no-exclude", "--exclude", "1"],
            vec!["deck", "--stage", "nope"],
            vec!["deck", "--prune", "nope"],
            vec!["deck", "--metric", "nope"],
            vec!["deck", "--exclude", ""],
            vec!["deck", "--exclude", "30257,,30301"],
            vec!["deck", "--fixed", "5"],
            vec!["deck", "--fixed", "x=1"],
            vec!["deck", "--top-k", "0"],
            vec!["deck", "--plan", "x", "--runs", "3"],
            vec!["bench", "--stage", "scan"],
            vec!["probe", "--card-id-min", "30251"],
            vec!["baseline", "--exclude", "30257"],
            vec!["compare", "--metric", "pt"],
        ] {
            let err = Args::parse(&argv(&bad)).expect_err(&format!("{bad:?} 应报错"));
            assert!(!format!("{err:#}").is_empty());
        }
    }

    /// 层1：`deck` 逐项取值与三态排除名单（默认 / 覆盖 / 清空）
    #[test]
    fn test_deck_parse() -> Result<()> {
        let args = Args::parse(&argv(&[
            "deck",
            "--stage",
            "compose",
            "--card-id-min",
            "30251",
            "--exclude",
            "30257,30301",
            "--seed-deck",
            "303124,303174,303044",
            "--runs",
            "10",
            "--seed",
            "7",
            "--metric",
            "pt",
            "--top-k",
            "3",
            "--slots",
            "6",
            "--fixed",
            "5=303054",
            "--fixed",
            "4=302894",
            "--max-per-type",
            "5",
            "--min-types",
            "4",
        ]))?;
        assert_eq!(
            args.cmd,
            Some(Cmd::Deck {
                plan: None,
                args: DeckArgs {
                    stage: Stage::Compose,
                    card_id_min: Some(30_251),
                    exclude: Some(vec![30_257, 30_301]),
                    seed_deck: Some(vec![303_124, 303_174, 303_044]),
                    runs: Some(10),
                    seed: Some(7),
                    metric: Some("pt".to_string()),
                    top_k: Some(3),
                    slots: Some(6),
                    fixed: vec![
                        FixedCard { slot: 5, idrank: 303_054 },
                        FixedCard { slot: 4, idrank: 302_894 },
                    ],
                    max_per_type: Some(5),
                    min_types: Some(4),
                    prune: None,
                    screen_runs: None,
                    fine_count: None,
                    holdout_seed: None,
                    holdout_count: None,
                    goal: None,
                    correct: None,
                }
            })
        );

        // 缺省：阶段 scan、排除名单走默认（`None`）、参考卡组走默认
        let args = Args::parse(&argv(&["deck"]))?;
        assert_eq!(
            args.cmd,
            Some(Cmd::Deck {
                plan: None,
                args: DeckArgs {
                    stage: Stage::Scan,
                    card_id_min: None,
                    exclude: None,
                    seed_deck: None,
                    runs: None,
                    seed: None,
                    metric: None,
                    top_k: None,
                    slots: None,
                    fixed: Vec::new(),
                    max_per_type: None,
                    min_types: None,
                    prune: None,
                    screen_runs: None,
                    fine_count: None,
                    holdout_seed: None,
                    holdout_count: None,
                    goal: None,
                    correct: None,
                }
            })
        );

        // `--no-exclude` 是显式空名单，与「没给」可区分
        let args = Args::parse(&argv(&["deck", "--no-exclude"]))?;
        match args.cmd {
            Some(Cmd::Deck { args: opts, .. }) => assert_eq!(opts.exclude, Some(Vec::new())),
            other => panic!("应为 deck 子命令: {other:?}"),
        }
        Ok(())
    }

    /// 层1：`deck --correct` 只认三档，且只对 scan / all 有意义；`--plan` 与实验开关互斥
    #[test]
    fn test_deck_correct_and_plan_rules() -> Result<()> {
        let args = Args::parse(&argv(&["deck", "--stage", "scan", "--correct", "holm"]))?;
        match args.cmd {
            Some(Cmd::Deck { args: opts, .. }) => assert_eq!(opts.correct, Some(Correct::Holm)),
            other => panic!("应为 deck 子命令: {other:?}"),
        }
        assert!(
            Args::parse(&argv(&["deck", "--correct", "nope"])).is_err(),
            "未知校正口径应报错"
        );
        assert!(
            Args::parse(&argv(&["deck", "--stage", "rank", "--correct", "holm"])).is_err(),
            "rank 阶段没有可比族，--correct 应报错而不是静默忽略"
        );
        assert!(
            Args::parse(&argv(&["deck", "--stage", "compose", "--correct", "holm"])).is_err(),
            "compose 阶段不出实测，--correct 应报错"
        );

        // 计划与实验开关互斥：计划路径自带、不参与实验变量
        let args = Args::parse(&argv(&["deck", "--plan", "exp.json"]))?;
        match &args.cmd {
            Some(Cmd::Deck { plan, args: opts }) => {
                assert_eq!(plan.as_deref(), Some(Path::new("exp.json")));
                assert_eq!(opts.stage, Stage::Scan, "计划模式下命令行侧的实验变量为空");
            }
            other => panic!("应为 deck 子命令: {other:?}"),
        }
        for bad in [
            vec!["deck", "--plan", "exp.json", "--runs", "10"],
            vec!["deck", "--plan", "exp.json", "--seed", "1"],
            vec!["deck", "--plan", "exp.json", "--stage", "all"],
            vec!["deck", "--plan", "exp.json", "--exclude", "30257"],
            vec!["deck", "--plan", "exp.json", "--metric", "pt"],
            vec!["deck", "--plan", "exp.json", "--correct", "holm"],
            vec!["cardtest", "--plan", "exp.json"],
        ] {
            let err = Args::parse(&argv(&bad)).expect_err(&format!("{bad:?} 应报错"));
            assert!(format!("{err:#}").contains("--plan"), "{bad:?}: {err:#}");
        }
        Ok(())
    }

    /// 层1：归属错误要指出是哪个选项（提示「用 -h」），不能只说「参数错」
    #[test]
    fn test_stray_option_is_named() {
        let err = Args::parse(&argv(&["probe", "--runs", "3", "--dir", "x"])).expect_err("probe 不认识这两个选项");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("probe") && msg.contains("--runs") && msg.contains("--dir"),
            "{msg}"
        );
        assert!(msg.contains("-h"), "错误文案应给用法提示: {msg}");
    }

    /// 层1：帮助不跑任何东西，只打用法
    #[test]
    fn test_help_prints_usage() -> Result<()> {
        let mut out = Vec::new();
        run_from_args(&argv(&["-h"]), &mut out)?;
        let text = String::from_utf8(out).expect("UTF-8");
        assert!(text.contains("umaai-tools"));
        assert!(text.contains("baseline"));
        assert!(text.contains("deck"));
        assert!(text.contains("--data-root"));
        Ok(())
    }

    /// 层1：`cardtest` 逐项取值与必给选项
    #[test]
    fn test_cardtest_parse() -> Result<()> {
        let args = Args::parse(&argv(&[
            "cardtest",
            "--base",
            "303124,303174,303044,303064,302894",
            "--slot",
            "3",
            "--cards",
            "303114",
            "--cards",
            "302754,302984",
            "--runs",
            "10",
            "--seed",
            "7",
            "--metric",
            "pt",
            "--goal",
            "50",
            "--allow-cross-type",
        ]))?;
        assert_eq!(
            args.cmd,
            Some(Cmd::Cardtest(CardTestArgs {
                base: vec![303_124, 303_174, 303_044, 303_064, 302_894],
                slot: Some(3),
                cards: vec![303_114, 302_754, 302_984],
                greedy: false,
                greedy_steps: None,
                runs: Some(10),
                seed: Some(7),
                metric: Some("pt".to_string()),
                goal: Some(50.0),
                correct: None,
                allow_cross_type: true,
            }))
        );

        // 贪婪：--slot 与 --greedy 互斥、--greedy-steps 需要 --greedy
        let args = Args::parse(&argv(&[
            "cardtest",
            "--base",
            "303124,303174,303044,303064,302894",
            "--cards",
            "303114",
            "--greedy",
            "--greedy-steps",
            "3",
            "--correct",
            "bonferroni",
        ]))?;
        match args.cmd {
            Some(Cmd::Cardtest(opts)) => {
                assert!(opts.greedy && opts.slot.is_none());
                assert_eq!(opts.greedy_steps, Some(3));
                assert_eq!(opts.correct, Some(Correct::Bonferroni));
            }
            other => panic!("应为 cardtest 子命令: {other:?}"),
        }

        // 必给选项缺失即报错；非法观测轴与零局在解析期就拦下
        for bad in [
            vec!["cardtest"],
            vec![
                "cardtest", "--base", "303124", "--cards", "303114", "--slot", "0", "--greedy",
            ],
            vec![
                "cardtest",
                "--base",
                "303124",
                "--cards",
                "303114",
                "--greedy-steps",
                "3",
            ],
            vec![
                "cardtest",
                "--base",
                "303124",
                "--cards",
                "303114",
                "--greedy",
                "--greedy-steps",
                "0",
            ],
            vec!["cardtest", "--base", "303124", "--cards", "303114", "--correct", "nope"],
            vec!["cardtest", "--base", "303124"],
            vec!["cardtest", "--base", "303124", "--slot", "0"],
            vec![
                "cardtest", "--base", "303124", "--slot", "0", "--cards", "303114", "--metric", "nope",
            ],
            vec![
                "cardtest", "--base", "303124", "--slot", "0", "--cards", "303114", "--goal", "abc",
            ],
            vec![
                "cardtest", "--base", "303124", "--slot", "0", "--cards", "303114", "--stage", "scan",
            ],
        ] {
            let err = Args::parse(&argv(&bad)).expect_err(&format!("{bad:?} 应报错"));
            assert!(!format!("{err:#}").is_empty());
        }
        Ok(())
    }

    /// 层1：机读行的值不含空白；五维用逗号连接
    #[test]
    fn test_join_and_token() {
        assert_eq!(join(&[1, 2, 3]), "1,2,3");
        let line = report::machine_line("T", &[("five", join(&[1, 2]))]);
        assert_eq!(line.split_whitespace().count(), 2, "值内不得出现空白: {line}");
    }
}
