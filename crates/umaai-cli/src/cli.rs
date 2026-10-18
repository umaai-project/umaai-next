//! 入口层：参数解析 + 选择器映射 + 运行装配
//!
//! 本仓 CLI 只认**无剧本基础局**（`scenario = "basic"` + `trainer = "random"`）：剧本装配
//! 是剧本仓自带 bin 的事（见 `docs/plan/miko_next_plan.md` §2「剧本仓形态」）。未知选择器
//! **报错并列出可用值**，不静默兜底——与配置层「未知键严格报错」同一口径。
//!
//! 入口逻辑落在库里而不在 `main.rs`：集成测试与下游可以直接调 [`run_from_args`]，
//! 输出目标由调用方注入。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Result, anyhow, bail};
use umaai::Config;
use umaai::runner::Runner;
use umaai_core::Data;
use umasim::game::base::basic::BasicGame;
use umasim::trainer::RandomTrainer;

use crate::frontend::{PrintFrontend, PrintOptions, Verbosity};
use crate::render::term;

/// 数据根的默认值（相对当前目录；发布形态的定位留打包阶段定）
pub const DEFAULT_DATA_ROOT: &str = "gamedata";

/// 覆盖层所在目录：约定为当前目录（覆盖层与默认层同名，靠目录区分角色）
pub const CONFIG_DIR: &str = ".";

/// 本仓 CLI 认识的无剧本基础局选择器
const BASIC_SCENARIO: &str = "basic";
/// 本仓 CLI 认识的契约参考训练员选择器
const RANDOM_TRAINER: &str = "random";

/// 命令行参数
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// 数据根（只读输入）
    pub data_root: PathBuf,
    /// 呈现宽度（`None` = 探测终端，探测不到用回退值）
    pub width: Option<usize>,
    /// 输出详细度
    pub verbosity: Verbosity,
    /// 关闭着色
    pub no_color: bool,
    /// 只打印用法
    pub help: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            data_root: PathBuf::from(DEFAULT_DATA_ROOT),
            width: None,
            verbosity: Verbosity::default(),
            no_color: false,
            help: false,
        }
    }
}

impl Args {
    /// 用法文本
    #[must_use]
    pub fn usage() -> &'static str {
        "umaai-cli —— 无剧本基础局冒烟（跑完一局并呈现过程与终局）

用法: umaai-cli [选项]

选项:
  --data-root <路径>  数据根（默认 gamedata，相对当前目录；内含默认层配置）
  --width <列数>      呈现宽度（默认探测终端，探测不到用 100；最小 40）
  --quiet             只出终局块
  --verbose           出每阶段行与决策明细
  --no-color          关闭着色
  -h, --help          打印本用法

配置:
  默认层 = <数据根>/ai_config.toml（字段齐全）；要改配置就在**当前目录**放一份同名
  ai_config.toml（覆盖层：同构、字段可选、段缺失整段沿用），存在才合并。

说明:
  一局局数由入口固定为单局（批量统计归 umaai-tools）；剧本选择器只认 basic，
  训练员只认 random，其余剧本由剧本仓自带的 bin 装配。"
    }

    /// 解析参数（`argv` 含程序名，与 `std::env::args` 一致）
    pub fn parse(argv: &[String]) -> Result<Self> {
        let mut args = Self::default();
        let mut it = argv.iter().skip(1);
        while let Some(flag) = it.next() {
            match flag.as_str() {
                "--data-root" => args.data_root = PathBuf::from(value(&mut it, "--data-root")?),
                "--width" => {
                    let raw = value(&mut it, "--width")?;
                    let cols: usize = raw.parse().map_err(|_| anyhow!("--width 需要正整数，收到 \"{raw}\""))?;
                    if cols < term::MIN_WIDTH {
                        bail!("--width 必须 ≥ {}（更窄会把表格里的 CJK 切坏）", term::MIN_WIDTH);
                    }
                    args.width = Some(cols);
                }
                "--quiet" => {
                    if args.verbosity == Verbosity::Verbose {
                        bail!("--quiet 与 --verbose 互斥");
                    }
                    args.verbosity = Verbosity::Quiet;
                }
                "--verbose" => {
                    if args.verbosity == Verbosity::Quiet {
                        bail!("--quiet 与 --verbose 互斥");
                    }
                    args.verbosity = Verbosity::Verbose;
                }
                "--no-color" => args.no_color = true,
                "-h" | "--help" => args.help = true,
                other => bail!("未知参数 \"{other}\"（用 -h 看用法）"),
            }
        }
        Ok(args)
    }
}

/// 取一个开关的值（缺值即报错）
fn value<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<&'a str> {
    it.next().map(String::as_str).ok_or_else(|| anyhow!("{flag} 缺少取值"))
}

/// 入口：解析参数并跑完一局（`argv` 含程序名；输出写入 `out`）
pub fn run_from_args(argv: &[String], out: &mut dyn Write) -> Result<()> {
    let args = Args::parse(argv)?;
    if args.help {
        writeln!(out, "{}", Args::usage())?;
        return Ok(());
    }
    run(&args, out)
}

/// 按参数装配并跑完一局
fn run(args: &Args, out: &mut dyn Write) -> Result<()> {
    // 配置先于静态数据：两份 ai_config.toml 都在数据加载之外（默认层在数据根、覆盖层在当前目录）
    let cfg = Config::load_with_overlay_file(&args.data_root, CONFIG_DIR)?;
    init_logger(&cfg.output.log_level)?;
    let overlay = Path::new(CONFIG_DIR).join(umaai::config::CONFIG_FILE);
    if overlay.exists() {
        log::info!(
            "配置覆盖层: {}（默认层: {}）",
            overlay.display(),
            args.data_root.join(umaai::config::CONFIG_FILE).display()
        );
    }
    Data::init(&args.data_root)?;
    let consts = &Data::get()?.consts;

    let opts = PrintOptions {
        verbosity: args.verbosity,
        width: term::resolve_width(args.width),
        color: term::use_color(args.no_color),
    };
    let mut fe = PrintFrontend::new(out, opts, consts);
    let game = build_game(&cfg)?;
    let trainer = build_trainer(&cfg)?;

    {
        let mut runner = Runner::new(&cfg, &mut fe)?;
        runner.run_game(game, &trainer, None)?;
    }
    if fe.write_failed() {
        bail!("输出写入失败（下游可能已提前关闭）");
    }
    Ok(())
}

/// 按 `sim.scenario` 装配一局（本仓只认无剧本基础局）
///
/// 入口与 `umaai-tools`（跑批 / 探针）**共用同一个装配点**：选择器口径只此一处，
/// 工具看到的局面与单局冒烟同源。
pub fn build_game(cfg: &Config) -> Result<BasicGame> {
    match cfg.sim.scenario.as_str() {
        BASIC_SCENARIO => BasicGame::newgame(cfg.sim.uma, &cfg.sim.cards, cfg.inherit(), cfg.race_grades()?),
        other => bail!("未知剧本 \"{other}\"（本仓 CLI 只认 {BASIC_SCENARIO}；具体剧本由剧本仓自带的 bin 装配）"),
    }
}

/// 按 `sim.trainer` 装配训练员（本仓只认契约参考实现）
pub fn build_trainer(cfg: &Config) -> Result<RandomTrainer> {
    match cfg.sim.trainer.as_str() {
        RANDOM_TRAINER => Ok(RandomTrainer),
        other => bail!("未知训练员 \"{other}\"（本仓 CLI 只认 {RANDOM_TRAINER}）"),
    }
}

/// 极简 stderr 日志器：消费 `output.log_level`
///
/// 引擎自己不打印，诊断经 `log` 门面与 `umaai_core::diag` 出来后落到这里；不装日志器时
/// 这些记录被静默丢弃。级别放在共享状态上，重复初始化（测试里多次调用）不报错且即时生效。
struct StderrLogger;

/// 生效的日志级别（`enabled` 每次读取，重装日志器后新级别也会作用到已装实例）
static LEVEL: Mutex<log::LevelFilter> = Mutex::new(log::LevelFilter::Off);

/// 当前生效的日志级别
fn level() -> log::LevelFilter {
    *LEVEL.lock().unwrap_or_else(|err| err.into_inner())
}

impl log::Log for StderrLogger {
    fn enabled(&self, meta: &log::Metadata<'_>) -> bool {
        meta.level() <= level()
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("[{:<5}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

/// 按级别名装日志器（级别名非法即报错，与配置层严格口径一致）
fn init_logger(level_name: &str) -> Result<()> {
    let filter = match level_name.to_ascii_lowercase().as_str() {
        "off" => log::LevelFilter::Off,
        "error" => log::LevelFilter::Error,
        "warn" => log::LevelFilter::Warn,
        "info" => log::LevelFilter::Info,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        other => bail!("output.log_level 非法 \"{other}\"（可用: off / error / warn / info / debug / trace）"),
    };
    *LEVEL.lock().unwrap_or_else(|err| err.into_inner()) = filter;
    log::set_max_level(filter);
    if filter == log::LevelFilter::Off {
        return Ok(());
    }
    // 已装过（重复调用）不是错误；级别走静态，重复装也能同步
    let _ = log::set_boxed_logger(Box::new(StderrLogger));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造 argv（含程序名）
    fn argv(rest: &[&str]) -> Vec<String> {
        std::iter::once("umaai-cli".to_string())
            .chain(rest.iter().map(|s| (*s).to_string()))
            .collect()
    }

    /// 层1：无参数即默认值
    #[test]
    fn test_parse_defaults() {
        let args = Args::parse(&argv(&[])).expect("默认参数应可解析");
        assert_eq!(args.data_root, PathBuf::from(DEFAULT_DATA_ROOT));
        assert_eq!(args.verbosity, Verbosity::Normal);
        assert!(args.width.is_none() && !args.no_color);
    }

    /// 层1：逐项取值
    #[test]
    fn test_parse_values() -> Result<()> {
        let args = Args::parse(&argv(&[
            "--data-root",
            "/tmp/d",
            "--width",
            "88",
            "--verbose",
            "--no-color",
        ]))?;
        assert_eq!(args.data_root, PathBuf::from("/tmp/d"));
        assert_eq!(args.width, Some(88));
        assert_eq!(args.verbosity, Verbosity::Verbose);
        assert!(args.no_color);
        Ok(())
    }

    /// 层1：未知参数、缺值、非法与过窄宽度、互斥开关都要报错
    #[test]
    fn test_parse_errors() {
        for bad in [
            vec!["--nope"],
            vec!["--data-root"],
            vec!["--width", "abc"],
            vec!["--width", "0"],
            vec!["--width", "39"],
            vec!["--quiet", "--verbose"],
        ] {
            let err = Args::parse(&argv(&bad)).expect_err(&format!("{bad:?} 应报错"));
            assert!(!format!("{err:#}").is_empty());
        }
    }

    /// 层1：帮助不跑对局（只打用法）
    #[test]
    fn test_help_prints_usage() -> Result<()> {
        let mut out = Vec::new();
        run_from_args(&argv(&["-h"]), &mut out)?;
        let text = String::from_utf8(out).expect("UTF-8");
        assert!(text.contains("umaai-cli"));
        assert!(text.contains("--data-root"));
        Ok(())
    }

    /// 层1：选择器严格口径 —— 未知剧本 / 训练员报错并列出可用值，不静默兜底
    ///
    /// 覆盖层没有命令行注入缝（约定文件在运行目录），故这里用文本入口
    /// [`Config::parse`] 构造坏配置，直接考 [`build_game`] / [`build_trainer`]。
    #[test]
    fn test_unknown_selectors_rejected() -> Result<()> {
        let base = std::fs::read_to_string(umaai_core::testing::data_root()?.join(umaai::config::CONFIG_FILE))?;

        let cfg = Config::parse(&base, Some("[sim]\nscenario = \"nope\"\n"))?;
        let msg = format!("{:#}", build_game(&cfg).unwrap_err());
        assert!(msg.contains("nope"), "错误里应出现写错的选择器: {msg}");
        assert!(msg.contains(BASIC_SCENARIO), "错误里应列出可用选择器: {msg}");

        let cfg = Config::parse(&base, Some("[sim]\ntrainer = \"nope\"\n"))?;
        let msg = format!("{:#}", build_trainer(&cfg).unwrap_err());
        assert!(msg.contains("nope"), "错误里应出现写错的训练员: {msg}");
        assert!(msg.contains(RANDOM_TRAINER), "错误里应列出可用训练员: {msg}");
        Ok(())
    }

    /// 层1：级别名非法即报错；重复初始化时新级别即时生效（含提升级别）
    #[test]
    fn test_init_logger_levels() {
        assert!(init_logger("info").is_ok());
        assert_eq!(level(), log::LevelFilter::Info);
        assert!(init_logger("noisy").is_err());
        assert_eq!(level(), log::LevelFilter::Info, "非法级别不得改动生效级别");
        assert!(init_logger("trace").is_ok());
        assert_eq!(level(), log::LevelFilter::Trace, "重复初始化应把级别抬上去");
        assert!(init_logger("off").is_ok());
        assert_eq!(level(), log::LevelFilter::Off);
    }
}
