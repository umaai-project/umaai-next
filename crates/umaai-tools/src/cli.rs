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

use crate::baseline::{self, Case, Difference};
use crate::{batch, probe, report};

/// 数据根默认值（相对当前目录；发布形态的定位留打包阶段定）
pub const DEFAULT_DATA_ROOT: &str = "gamedata";

/// 基线目录默认值（相对 workspace 根）
pub const DEFAULT_BASELINE_DIR: &str = "crates/umaai-tests/baselines";

/// 子命令与各自的选项
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

/// 命令行参数
#[derive(Debug, Clone, PartialEq, Eq)]
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
  --update            重录（跑法不变，只刷新期望值与元信息）"
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
                let bad = stray(&[
                    ("--search-n", self.search_n.is_some()),
                    ("--dir", self.dir.is_some()),
                    ("--only", !self.only.is_empty()),
                    ("--update", self.update),
                ]);
                ensure!(bad.is_empty(), "bench 不认识 {}（用 -h 看用法）", bad.join(" / "));
                Cmd::Bench {
                    runs: self.runs,
                    seed: self.seed,
                }
            }
            "probe" => {
                let bad = stray(&[
                    ("--runs", self.runs.is_some()),
                    ("--dir", self.dir.is_some()),
                    ("--only", !self.only.is_empty()),
                    ("--update", self.update),
                ]);
                ensure!(bad.is_empty(), "probe 不认识 {}（用 -h 看用法）", bad.join(" / "));
                Cmd::Probe {
                    seed: self.seed,
                    search_n: self.search_n,
                }
            }
            "baseline" => {
                let bad = stray(&[
                    ("--runs", self.runs.is_some()),
                    ("--seed", self.seed.is_some()),
                    ("--search-n", self.search_n.is_some()),
                ]);
                ensure!(bad.is_empty(), "baseline 不认识 {}（用 -h 看用法）", bad.join(" / "));
                Cmd::Baseline {
                    dir: self.dir.unwrap_or_else(|| PathBuf::from(DEFAULT_BASELINE_DIR)),
                    only: self.only,
                    update: self.update,
                }
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
    }
}

/// 子命令不认识的选项名（按声明顺序）
fn stray(pairs: &[(&'static str, bool)]) -> Vec<&'static str> {
    pairs.iter().filter(|(_, on)| *on).map(|(n, _)| *n).collect()
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

/// 整数数组转逗号分隔（机读行与表格共用；不使用 `{:?}`，避免空格破坏切分）
fn join(v: &[i32]) -> String {
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
            vec!["--quiet"],
            vec!["--data-root", "/tmp/d"],
            vec!["bench", "--runs"],
            vec!["bench", "--runs", "abc"],
            vec!["--width", "39"],
            vec!["--width", "0"],
        ] {
            let err = Args::parse(&argv(&bad)).expect_err(&format!("{bad:?} 应报错"));
            assert!(!format!("{err:#}").is_empty());
        }
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
        assert!(text.contains("--data-root"));
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
