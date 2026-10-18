//! L3：`umaai-tools` 的跑批 / 探针 / 入口链冒烟
//!
//! 覆盖：跑批的逐局一致性与分布口径、探针的预算与候选对齐、入口层的机读行与子命令
//! 分发（含基线校验路径）。不设数值基线——工具有没有量对是这里的判据，量到多少归 L4。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use umaai::Config;
use umaai_core::Data;
use umaai_core::testing::{data_root, workspace_root};
use umaai_tests::Checker;
use umaai_tools::cli;
use umaai_tools::{batch, probe};

/// 探针冒烟的搜索预算（只要够小就跑得快）
const SEARCH_N: usize = 4;

/// 测试用的宿主目录（`target/` 下，gitignore 覆盖）
fn host_dir(tag: &str) -> Result<PathBuf> {
    let dir = workspace_root()?.join("target/test-tmp").join(tag);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 载入配置并初始化静态数据（与 `umaai-tools` 入口同一条链；宿主目录注入，不随运行目录漂移）
fn setup(host: &Path) -> Result<Config> {
    let root = data_root()?;
    Data::init(&root).context("载入数据根失败")?;
    Config::load_with_overlay_file(&root, host).context("载入配置失败")
}

/// 构造带程序名的 argv
fn argv(rest: &[String]) -> Vec<String> {
    std::iter::once("umaai-tools".to_string())
        .chain(rest.iter().cloned())
        .collect()
}

#[test]
fn test_batch_and_probe() -> Result<()> {
    let cfg = setup(&host_dir("tools-smoke-host")?)?;
    let mut checker = Checker::new("umaai-tools 跑批与探针");

    // 跑批：同参数两次必须逐位一致（工具的量具本身要可复现）
    let spec = batch::Spec { runs: 2, seed: 42 };
    let a = batch::run(&cfg, &spec)?;
    let b = batch::run(&cfg, &spec)?;
    let scores = |r: &batch::Report| r.runs.iter().map(|x| x.summary.score).collect::<Vec<_>>();
    println!("跑批评分序列 = {:?}", scores(&a));
    println!(
        "分布: mean={:.2} median={:.2} min={:.2} max={:.2} stdev={:.2} pt_mean={:.2} 吞吐={:.2}/s",
        a.dist.mean,
        a.dist.median,
        a.dist.min,
        a.dist.max,
        a.dist.stdev,
        a.dist.pt_mean,
        a.throughput()
    );
    checker.check(a.runs.len() == 2, "局数与规格一致");
    checker.check(
        a.runs.iter().map(|r| r.seed).collect::<Vec<_>>() == vec![42, 43],
        "第 i 局种子为 seed + i",
    );
    checker.check(scores(&a) == scores(&b), "同参数两次跑批逐位一致");
    checker.check(
        a.dist.min <= a.dist.mean && a.dist.mean <= a.dist.max,
        "分布均值落在极值区间内",
    );
    checker.check(a.throughput() > 0.0, "吞吐为正");
    checker.check(batch::run(&cfg, &batch::Spec { runs: 0, seed: 0 }).is_err(), "零局报错");

    // 探针：预算逐候选对齐、成功 + 失败 = 计划、候选数与均分同长
    let mut cfg = cfg;
    cfg.search.search_n = SEARCH_N;
    let rep = probe::search_root(&cfg, 42)?;
    println!(
        "探针: turn={} stage_key={} 候选={} 计划={} 成功={} 失败={} 中选=[{}] {} 根评分={:.2} 耗时={:?}",
        rep.turn,
        rep.stage_key,
        rep.candidates,
        rep.total_planned(),
        rep.total_succeeded(),
        rep.total_failed(),
        rep.best_idx,
        rep.best_action,
        rep.root_score,
        rep.elapsed
    );
    checker.check(
        rep.candidates > 0 && rep.candidates == rep.means.len(),
        "候选数与均分同长",
    );
    checker.check(rep.planned.iter().all(|&n| n == SEARCH_N), "每候选计划次数等于搜索预算");
    checker.check(
        rep.succeeded.iter().zip(rep.planned.iter()).all(|(s, p)| s == p),
        "基础局 rollout 不应失败",
    );
    checker.check(rep.best_idx < rep.candidates, "中选下标落在候选范围内");
    checker.check(rep.root_score.is_finite(), "根局面评分有限");

    // 同种子重复探针必须逐位一致
    let again = probe::search_root(&cfg, 42)?;
    checker.check(
        rep.means == again.means && rep.best_idx == again.best_idx,
        "同种子两次探针逐位一致",
    );

    checker.finish()
}

#[test]
fn test_entry_layer() -> Result<()> {
    let host = host_dir("tools-entry-host")?;
    setup(&host)?;
    let mut checker = Checker::new("umaai-tools 入口层");

    // bench --quiet：只出一行机读结果
    let root = data_root()?;
    let mut out = Vec::new();
    cli::run_from_args_with_host_dir(
        &argv(&[
            "bench".to_string(),
            "--runs".to_string(),
            "1".to_string(),
            "--quiet".to_string(),
            "--data-root".to_string(),
            root.display().to_string(),
        ]),
        &mut out,
        &host,
    )?;
    let text = String::from_utf8(out).context("输出应为 UTF-8")?;
    println!("bench 机读输出 = {}", text.trim_end());
    checker.check(text.lines().count() == 1, "--quiet 只出一行");
    checker.check(text.starts_with("BENCH "), "机读行以 BENCH 起头");
    checker.check(
        text.split_whitespace().skip(1).all(|f| f.contains('=')),
        "机读行各字段形如 key=value",
    );

    // baseline 校验路径：只跑一份，确认入口分发到判定并给出结论行
    let mut out = Vec::new();
    cli::run_from_args_with_host_dir(
        &argv(&[
            "baseline".to_string(),
            "--only".to_string(),
            "basic_full_run".to_string(),
            "--data-root".to_string(),
            root.display().to_string(),
            "--dir".to_string(),
            workspace_root()?
                .join("crates/umaai-tests/baselines")
                .display()
                .to_string(),
        ]),
        &mut out,
        &host,
    )?;
    let text = String::from_utf8(out).context("输出应为 UTF-8")?;
    println!("baseline 输出 = {}", text.trim_end());
    checker.check(text.contains("ok   basic_full_run"), "校验通过并逐份打印结论");
    checker.check(
        text.contains("BASELINE ") && text.contains("failed=0"),
        "总结行为机读格式且失败数为 0",
    );

    // 覆盖层从注入的宿主目录读：坏覆盖层在入口即报错（与主线同一条装配链）
    let bad = host.join("bad");
    std::fs::create_dir_all(&bad)?;
    std::fs::write(bad.join("ai_config.toml"), "[sim]\nume = 1\n")?;
    let err = cli::run_from_args_with_host_dir(
        &argv(&[
            "bench".to_string(),
            "--runs".to_string(),
            "1".to_string(),
            "--data-root".to_string(),
            root.display().to_string(),
        ]),
        &mut Vec::new(),
        &bad,
    );
    let msg = err.as_ref().map_or_else(|e| format!("{e:#}"), |_| String::new());
    checker.check(
        err.is_err() && msg.contains("ume"),
        &format!("坏覆盖层在入口即报错: {msg}"),
    );

    // 未知子命令与只打用法
    checker.check(
        cli::run_from_args_with_host_dir(&argv(&["nope".to_string()]), &mut Vec::new(), &host).is_err(),
        "未知子命令报错",
    );

    // 零预算探针直接拒绝（不给「0 次搜索」这种无意义输入留后门）
    checker.check(
        cli::run_from_args_with_host_dir(
            &argv(&[
                "probe".to_string(),
                "--search-n".to_string(),
                "0".to_string(),
                "--data-root".to_string(),
                root.display().to_string(),
            ]),
            &mut Vec::new(),
            &host,
        )
        .is_err(),
        "零预算探针报错",
    );

    let mut out = Vec::new();
    cli::run_from_args_with_host_dir(&argv(&["-h".to_string()]), &mut out, &host)?;
    checker.check(String::from_utf8_lossy(&out).contains("baseline"), "帮助列出子命令");

    checker.finish()
}
