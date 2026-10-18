//! L3 端到端：`umaai-cli` 的装配链与呈现（无剧本基础局跑满一局）
//!
//! 检查点：装配链（配置 → 静态数据 → 选择器映射 → Runner → 呈现）跑通、跑满 78 回合、
//! 终局块含评分与档次名、同参数两次运行**逐字节一致**（呈现可复现）、三档详细度各自成形、
//! 未知剧本选择器报错而不是静默兜底。
//!
//! 用 `--width` 固定宽度、`--no-color` 关色，输出才可逐字节比对（终端探测与 ANSI 都不确定）。

use std::path::PathBuf;

use anyhow::Result;
use umaai_core::testing::{data_root, workspace_root};
use umaai_tests::Checker;

/// 组装 argv（含程序名）
fn argv(extra: &[&str]) -> Vec<String> {
    let root = data_root().expect("定位数据根");
    let mut args = vec![
        "umaai-cli".to_string(),
        "--data-root".to_string(),
        root.display().to_string(),
        "--width".to_string(),
        "100".to_string(),
        "--no-color".to_string(),
    ];
    args.extend(extra.iter().map(|s| (*s).to_string()));
    args
}

/// 测试用的宿主目录（`target/` 下，gitignore 覆盖；覆盖层与默认层同名、靠目录区分）
fn host_dir(tag: &str) -> Result<PathBuf> {
    let dir = workspace_root()?.join("target/test-tmp").join(tag);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 跑一次并把输出收成字符串（宿主目录注入空目录，不受运行目录污染）
fn run(extra: &[&str]) -> Result<String> {
    let mut out = Vec::new();
    umaai_cli::cli::run_from_args_with_host_dir(&argv(extra), &mut out, &host_dir("cli-smoke-host")?)?;
    String::from_utf8(out).map_err(|err| anyhow::anyhow!("输出非 UTF-8: {err}"))
}

/// 回合行（`T0   …`）的条数
fn turn_lines(text: &str) -> usize {
    text.lines()
        .filter(|line| line.starts_with('T') && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit()))
        .count()
}

/// 端到端：跑满一局 + 终端块 + 可复现
#[test]
fn cli_full_game_smoke() -> Result<()> {
    let mut c = Checker::new("umaai-cli 无剧本基础局端到端");
    let text = run(&[])?;

    c.check(turn_lines(&text) == 78, "默认档落 78 个回合行（回合 0-77）");
    c.check(text.contains("终局"), "出终局块");
    c.check(text.contains("77/77"), "终局回合为 77/77（跑满）");
    c.check(!text.contains('\u{1b}'), "--no-color 下不得含 ANSI 转义");

    let again = run(&[])?;
    c.check(text == again, "同参数两次运行逐字节一致");

    println!("--- 默认档输出（尾部 20 行）---");
    for line in text.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev() {
        println!("{line}");
    }
    c.finish()
}

/// 端到端：三档详细度各自成形
#[test]
fn cli_verbosity_levels() -> Result<()> {
    let mut c = Checker::new("umaai-cli 详细度分档");

    let quiet = run(&["--quiet"])?;
    c.check(!quiet.contains("── 运行"), "静默档不出运行头");
    c.check(turn_lines(&quiet) == 0, "静默档不出回合行");
    c.check(quiet.contains("终局"), "静默档仍出终局块");

    let normal = run(&[])?;
    let verbose = run(&["--verbose"])?;
    c.check(verbose.contains("stage#"), "详细档出阶段行");
    c.check(
        turn_lines(&verbose) > turn_lines(&normal),
        "详细档行数多于默认档（阶段粒度）",
    );

    c.finish()
}

/// 端到端：装配链守门 —— 覆盖层不污染用例、数据根缺失与未知参数都报错
///
/// 覆盖层的注入缝是宿主目录参数：本用例一律注入空目录；运行目录（workspace 根）另设断言，
/// 防止约定覆盖层反过来污染未注入宿主目录的用例。选择器严格口径仍留层1 文本入口用例。
#[test]
fn cli_assembly_guards() -> Result<()> {
    let mut c = Checker::new("umaai-cli 装配链守门");
    let host = host_dir("cli-guard-host")?;

    c.check(
        !std::path::Path::new("ai_config.toml").exists(),
        "运行目录（workspace 根）不得存在 ai_config.toml：约定覆盖层会污染未注入宿主目录的用例",
    );

    // 数据根不存在 → 报错而非 panic
    let mut out = Vec::new();
    let bad = vec![
        "umaai-cli".to_string(),
        "--data-root".to_string(),
        "/nonexistent/umaai-data-root".to_string(),
    ];
    c.check(
        umaai_cli::cli::run_from_args_with_host_dir(&bad, &mut out, &host).is_err(),
        "数据根缺失应报错",
    );

    // 未知参数 → 报错
    let mut out = Vec::new();
    c.check(
        umaai_cli::cli::run_from_args_with_host_dir(&["umaai-cli".to_string(), "--nope".to_string()], &mut out, &host)
            .is_err(),
        "未知参数应报错",
    );

    c.finish()
}

/// 端到端：覆盖层从**注入的宿主目录**读 —— 临时目录构造坏覆盖层，运行目录零污染
#[test]
fn cli_overlay_from_injected_host_dir() -> Result<()> {
    let mut c = Checker::new("umaai-cli 覆盖层注入");
    let host = host_dir("cli-overlay-host")?;
    let empty = host.join("empty");
    let bad = host.join("bad");
    std::fs::create_dir_all(&empty)?;
    std::fs::create_dir_all(&bad)?;
    std::fs::write(bad.join("ai_config.toml"), "[sim]\nscenario = \"nope\"\n")?;

    // 坏覆盖层：报错并点名写错的选择器
    let mut out = Vec::new();
    let err = umaai_cli::cli::run_from_args_with_host_dir(&argv(&[]), &mut out, &bad);
    let msg = err.as_ref().map_or_else(|e| format!("{e:#}"), |_| String::new());
    c.check(
        err.is_err() && msg.contains("nope"),
        &format!("坏覆盖层报错并点名选择器: {msg}"),
    );

    // 同一 argv 换空宿主目录即正常跑完：覆盖层只在注入目录里找
    let ok = umaai_cli::cli::run_from_args_with_host_dir(&argv(&[]), &mut Vec::new(), &empty);
    c.check(ok.is_ok(), "空宿主目录下同一 argv 正常跑完");

    std::fs::remove_dir_all(&host)?;
    c.finish()
}
