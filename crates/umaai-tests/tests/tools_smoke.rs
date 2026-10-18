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

#[test]
fn test_compare() -> Result<()> {
    let host = host_dir("tools-compare-host")?;
    let root = data_root()?;
    let mut checker = Checker::new("umaai-tools 分支推进对照");

    // 对照计划：探测回合 5、探测动作 1、观测 3 回合（自持 JSON，与入口同一条链）
    let plan = host.join("plan.json");
    std::fs::write(
        &plan,
        r#"{"seed": 42, "turn": 5, "action": 1, "turns": 3, "note": "冒烟"}"#,
    )?;

    let run_once = |out: &mut Vec<u8>| -> Result<()> {
        cli::run_from_args_with_host_dir(
            &argv(&[
                "compare".to_string(),
                "--plan".to_string(),
                plan.display().to_string(),
                "--data-root".to_string(),
                root.display().to_string(),
            ]),
            out,
            &host,
        )
    };

    let mut out = Vec::new();
    run_once(&mut out)?;
    let text = String::from_utf8(out).context("输出应为 UTF-8")?;
    println!("--- compare 输出（尾部 12 行）---");
    for line in text.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev() {
        println!("{line}");
    }
    let machine = text
        .lines()
        .find(|line| line.starts_with("COMPARE "))
        .unwrap_or_default()
        .to_string();
    checker.check(!machine.is_empty(), "出 COMPARE 机读行");
    checker.check(
        machine.contains("probe_turn=5") && machine.contains("turns=3"),
        "机读行带探测坐标与回合数",
    );
    checker.check(
        machine.contains("a_action=") && machine.contains("b_action="),
        "机读行带两支动作",
    );
    checker.check(
        machine.contains("a_turns=3") && machine.contains("b_turns=3"),
        "两支观测回合数一致",
    );
    checker.check(text.contains("Δ评分"), "人读表带差值列");

    // 同参数两次必须逐位一致（工具本身要可复现）
    let mut again = Vec::new();
    run_once(&mut again)?;
    checker.check(String::from_utf8_lossy(&again) == text, "同参数两次逐位一致");

    // 探测动作越界：点名候选数即报错
    let bad_action = host.join("bad_action.json");
    std::fs::write(&bad_action, r#"{"turn": 5, "action": 99, "turns": 3}"#)?;
    let err = cli::run_from_args_with_host_dir(
        &argv(&[
            "compare".to_string(),
            "--plan".to_string(),
            bad_action.display().to_string(),
            "--data-root".to_string(),
            root.display().to_string(),
        ]),
        &mut Vec::new(),
        &host,
    );
    let msg = err.as_ref().map_or_else(|e| format!("{e:#}"), |_| String::new());
    checker.check(
        err.is_err() && msg.contains("越界"),
        &format!("探测动作越界报错: {msg}"),
    );

    // 计划自检：零回合直接拒绝
    let bad_plan = host.join("bad_plan.json");
    std::fs::write(&bad_plan, r#"{"turn": 5, "action": 1, "turns": 0}"#)?;
    checker.check(
        cli::run_from_args_with_host_dir(
            &argv(&[
                "compare".to_string(),
                "--plan".to_string(),
                bad_plan.display().to_string(),
                "--data-root".to_string(),
                root.display().to_string(),
            ]),
            &mut Vec::new(),
            &host,
        )
        .is_err(),
        "零回合计划报错",
    );

    checker.finish()
}

#[test]
fn test_cardtest_tool() -> Result<()> {
    let host = host_dir("tools-cardtest-host")?;
    setup(&host)?;
    let root = data_root()?;
    let mut checker = Checker::new("umaai-tools 测卡工具");

    let run = |rest: &[&str]| -> Result<String> {
        let mut argv_full: Vec<String> = rest.iter().map(|s| (*s).to_string()).collect();
        argv_full.push("--data-root".to_string());
        argv_full.push(root.display().to_string());
        let mut out = Vec::new();
        cli::run_from_args_with_host_dir(&argv(&argv_full), &mut out, &host)?;
        String::from_utf8(out).context("输出应为 UTF-8")
    };
    // 默认参考卡组（速2耐1智2），固定其余 4 张、只换速槽
    let base = "303124,303174,303044,303064,302894";
    let args = [
        "cardtest",
        "--base",
        base,
        "--slot",
        "0",
        "--cards",
        "303114,302754,303124,302984",
        "--runs",
        "40",
        "--quiet",
    ];

    let out = run(&args)?;
    println!("cardtest 机读输出 =\n{}", out.trim_end());
    let head = out.lines().next().unwrap_or_default();
    checker.check(head.starts_with("CARDTEST "), "首行是 CARDTEST");
    checker.check(field(head, "slot").as_deref() == Some("0"), "变动槽位");
    checker.check(field(head, "candidates").as_deref() == Some("4"), "候选数");
    let rows: Vec<&str> = out.lines().filter(|l| l.starts_with("CARDTEST_ROW ")).collect();
    checker.check(rows.len() == 4, "每个候选一行");
    checker.check(
        rows.iter()
            .all(|l| field(l, "need").is_some() && field(l, "d_sd").is_some()),
        "每行都带配对标准差与所需局数建议",
    );
    // 候选里含基准卡本身 → 零差分自证行（reference=1）
    let reference = rows.iter().find(|l| field(l, "reference").as_deref() == Some("1"));
    checker.check(
        reference.is_some_and(|l| field(l, "d_mean").as_deref() == Some("0.00")),
        "基准卡自身的配对差分为零",
    );
    // 同参数两次逐位一致（排除耗时）
    let again = run(&args)?;
    checker.check(
        without_elapsed(&again) == without_elapsed(&out),
        "同参数两次测卡逐位一致（排除耗时）",
    );

    // 守卫：跨类型、重复卡组、过短基准、槽位越界、候选无法解析
    let err_of = |rest: &[&str]| -> String { run(rest).err().map(|err| format!("{err:#}")).unwrap_or_default() };
    let cross = err_of(&[
        "cardtest", "--base", base, "--slot", "0", "--cards", "302814", "--runs", "2",
    ]);
    checker.check(
        cross.contains("构成效应"),
        &format!("跨类型候选要点明测的是构成效应: {cross}"),
    );
    let cross_ok = run(&[
        "cardtest",
        "--base",
        base,
        "--slot",
        "0",
        "--cards",
        "302814",
        "--runs",
        "2",
        "--allow-cross-type",
        "--quiet",
    ]);
    checker.check(
        cross_ok.is_ok(),
        &format!(
            "--allow-cross-type 应放行跨类型候选: {:?}",
            cross_ok.as_ref().err().map(|e| format!("{e:#}"))
        ),
    );
    let dup = err_of(&[
        "cardtest", "--base", base, "--slot", "0", "--cards", "303174", "--runs", "2",
    ]);
    checker.check(dup.contains("重复卡组"), &format!("已占其他槽位的候选要拒绝: {dup}"));
    let short = err_of(&[
        "cardtest",
        "--base",
        "303124,303174",
        "--slot",
        "0",
        "--cards",
        "303114",
        "--runs",
        "2",
    ]);
    checker.check(short.contains("至少 5 张"), &format!("过短基准卡组要拒绝: {short}"));
    let oob = err_of(&[
        "cardtest", "--base", base, "--slot", "9", "--cards", "303114", "--runs", "2",
    ]);
    checker.check(oob.contains("越界"), &format!("槽位越界要拒绝: {oob}"));
    let unknown = err_of(&[
        "cardtest", "--base", base, "--slot", "0", "--cards", "999994", "--runs", "2",
    ]);
    checker.check(unknown.contains("无法解析"), &format!("未知候选要拒绝: {unknown}"));
    // 缺必给选项
    for bad in [
        vec!["cardtest"],
        vec!["cardtest", "--base", base],
        vec!["cardtest", "--base", base, "--slot", "0"],
    ] {
        let argv_full: Vec<String> = bad.iter().map(|s| (*s).to_string()).collect();
        let err = cli::run_from_args_with_host_dir(&argv(&argv_full), &mut Vec::new(), &host)
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap_or_default();
        checker.check(!err.is_empty(), &format!("{bad:?} 应报错: {err}"));
    }

    // 多重比较校正：缺省不校正（不出校正列），bonferroni 出校正 CI 且更宽
    checker.check(field(head, "correct").as_deref() == Some("none"), "缺省校正口径是 none");
    checker.check(
        rows.iter().all(|l| field(l, "d_ci_adj").is_none()),
        "不校正时不出校正列（默认报告口径不变）",
    );
    let corrected = run(&[
        "cardtest",
        "--base",
        base,
        "--slot",
        "0",
        "--cards",
        "303114,302754,303124,302984",
        "--runs",
        "40",
        "--correct",
        "bonferroni",
        "--quiet",
    ])?;
    println!("cardtest 校正输出 =\n{}", corrected.trim_end());
    let chead = corrected.lines().next().unwrap_or_default();
    checker.check(
        field(chead, "correct").as_deref() == Some("bonferroni"),
        "机读行点名校正口径",
    );
    let crows: Vec<&str> = corrected.lines().filter(|l| l.starts_with("CARDTEST_ROW ")).collect();
    checker.check(crows.len() == 4, "校正后仍是每候选一行");
    let num = |line: &str, key: &str| -> f64 { field(line, key).and_then(|v| v.parse().ok()).unwrap_or(f64::NAN) };
    checker.check(
        crows
            .iter()
            .all(|l| field(l, "d_ci_adj").is_some() && field(l, "p_adj").is_some() && field(l, "beyond").is_some()),
        "校正后每行带校正 CI / 校正 p / 越过零判定",
    );
    checker.check(
        crows.iter().all(|l| num(l, "d_ci_adj") >= num(l, "d_ci95") - 1e-9),
        "同族 4 个比较，Bonferroni 的置信半径不小于未校正半径",
    );

    checker.finish()
}

#[test]
fn test_cardtest_greedy() -> Result<()> {
    let host = host_dir("tools-cardtest-greedy-host")?;
    setup(&host)?;
    let root = data_root()?;
    let mut checker = Checker::new("umaai-tools 逐槽贪婪测卡");

    let run = |rest: &[&str]| -> Result<String> {
        let mut argv_full: Vec<String> = rest.iter().map(|s| (*s).to_string()).collect();
        argv_full.push("--data-root".to_string());
        argv_full.push(root.display().to_string());
        let mut out = Vec::new();
        cli::run_from_args_with_host_dir(&argv(&argv_full), &mut out, &host)?;
        String::from_utf8(out).context("输出应为 UTF-8")
    };

    // 默认参考卡组（速速耐智智），候选全是速卡：可放槽位 = 速槽 0 / 1
    let base = "303124,303174,303044,303064,302894";
    let args = [
        "cardtest",
        "--base",
        base,
        "--cards",
        "303114,302754,302984",
        "--greedy",
        "--runs",
        "10",
        "--quiet",
    ];
    let out = run(&args)?;
    println!("greedy 机读输出 =\n{}", out.trim_end());
    let head = out.lines().next().unwrap_or_default();
    checker.check(head.starts_with("CARDTEST_GREEDY "), "首行是 CARDTEST_GREEDY");
    checker.check(field(head, "base").as_deref() == Some(base), "基准卡组回显");
    checker.check(field(head, "correct").as_deref() == Some("none"), "缺省不校正");
    let steps: Vec<&str> = out.lines().filter(|l| l.starts_with("CARDTEST_STEP ")).collect();
    let step_count: usize = field(head, "steps").and_then(|v| v.parse().ok()).unwrap_or(0);
    checker.check(!steps.is_empty() && steps.len() == step_count, "逐步机读行与步数一致");
    checker.check(
        field(head, "stop").is_some(),
        &format!("停止原因要点名: {}", field(head, "stop").unwrap_or_default()),
    );
    let pairs: Vec<&str> = out.lines().filter(|l| l.starts_with("CARDTEST_PAIR ")).collect();
    checker.check(!pairs.is_empty(), "最后一步的组合排行要留证（为什么停下 / 继续）");
    // 采纳的替换都落在速槽（默认参考卡组 = 速速耐智智 → 速槽 0 / 1），卡组长度不变
    let final_deck = field(head, "final").unwrap_or_default();
    checker.check(
        final_deck.split(',').count() == 5,
        &format!("最终卡组仍是 5 张: {final_deck}"),
    );
    checker.check(
        steps
            .iter()
            .all(|l| field(l, "slot").is_some_and(|s| s == "-" || s == "0" || s == "1")),
        "候选全是速卡，替换只可能落在速槽 0 / 1",
    );
    let chosen = steps
        .iter()
        .filter(|l| field(l, "card").is_some_and(|c| c != "-"))
        .count();
    checker.check(
        chosen == 0 || final_deck != base,
        "一旦有采纳的替换，最终卡组必须与基准不同",
    );

    // 同参数两次逐位一致（排除耗时）
    let again = run(&args)?;
    checker.check(
        without_elapsed(&again) == without_elapsed(&out),
        "同参数两次贪婪逐位一致（排除耗时）",
    );

    // 守卫：候选重复（会重复测量并虚增校正族）、步数上限为 0、候选类型全不可放
    let err_of = |rest: &[&str]| -> String { run(rest).err().map(|err| format!("{err:#}")).unwrap_or_default() };
    let dup = err_of(&[
        "cardtest",
        "--base",
        base,
        "--cards",
        "303114,303114",
        "--greedy",
        "--runs",
        "2",
    ]);
    checker.check(dup.contains("重复 idrank"), &format!("重复候选要拒绝: {dup}"));
    let zero = err_of(&[
        "cardtest",
        "--base",
        base,
        "--cards",
        "303114",
        "--greedy",
        "--greedy-steps",
        "0",
        "--runs",
        "2",
    ]);
    checker.check(!zero.is_empty(), &format!("步数上限 0 要拒绝: {zero}"));
    // 友人候选在默认口径下放不进训练槽位（跨类型测的是构成效应，需显式放行）
    let friend = err_of(&[
        "cardtest", "--base", base, "--cards", "303054", "--greedy", "--runs", "2",
    ]);
    checker.check(
        friend.contains("没有任何可放"),
        &format!("无可放组合要点名原因: {friend}"),
    );

    checker.finish()
}

#[test]
fn test_deck_plan() -> Result<()> {
    let host = host_dir("tools-deck-plan-host")?;
    setup(&host)?;
    let root = data_root()?;
    let mut checker = Checker::new("umaai-tools 自持实验计划");

    let run = |rest: &[&str]| -> Result<String> {
        let mut argv_full: Vec<String> = rest.iter().map(|s| (*s).to_string()).collect();
        argv_full.push("--data-root".to_string());
        argv_full.push(root.display().to_string());
        let mut out = Vec::new();
        cli::run_from_args_with_host_dir(&argv(&argv_full), &mut out, &host)?;
        String::from_utf8(out).context("输出应为 UTF-8")
    };
    let write_plan = |name: &str, text: &str| -> Result<PathBuf> {
        let path = host.join(name);
        std::fs::write(&path, text).with_context(|| format!("写计划失败: {}", path.display()))?;
        Ok(path)
    };

    // 计划自持：compose 阶段（不跑局，秒级）
    let plan = write_plan(
        "compose.json",
        r#"{"stage": "compose", "seed": 7, "top_k": 2, "exclude": [], "note": "冒烟：清空排除名单"}"#,
    )?;
    let out = run(&["deck", "--plan", &plan.display().to_string(), "--quiet"])?;
    println!("plan compose 机读输出 = {}", out.trim_end());
    let line = out.lines().next().unwrap_or_default();
    checker.check(line.starts_with("DECK_COMPOSE "), "计划驱动 compose 出机读行");
    checker.check(
        field(line, "plan").is_some_and(|v| v.contains("compose.json")),
        "机读行回显计划路径",
    );
    checker.check(
        field(line, "top_k").as_deref() == Some("2"),
        "计划里的实验变量生效（top_k）",
    );
    // 计划里 exclude 为空数组 = 清空排除名单，与命令行 --no-exclude 同义
    let scanned = run(&[
        "deck",
        "--plan",
        &plan.display().to_string(),
        "--stage",
        "scan",
        "--runs",
        "2",
    ]);
    checker.check(scanned.is_err(), "计划与实验开关同时给要报错（不做覆盖合并）");

    // scan 阶段 + 校正口径也由计划自持
    let scan_plan = write_plan(
        "scan.json",
        r#"{"stage": "scan", "seed": 11, "runs": 2, "metric": "pt", "correct": "holm"}"#,
    )?;
    let out = run(&["deck", "--plan", &scan_plan.display().to_string(), "--quiet"])?;
    let line = out.lines().next().unwrap_or_default();
    checker.check(line.starts_with("DECK_SCAN "), "计划驱动 scan 出机读行");
    checker.check(field(line, "runs").as_deref() == Some("2"), "计划里的局数生效");
    checker.check(field(line, "seed").as_deref() == Some("11"), "计划里的种子生效");
    checker.check(field(line, "metric").as_deref() == Some("pt"), "计划里的观测轴生效");
    checker.check(
        field(line, "correct").as_deref() == Some("holm"),
        "计划里的校正口径生效",
    );
    // 同参数两次逐位一致
    let again = run(&["deck", "--plan", &scan_plan.display().to_string(), "--quiet"])?;
    checker.check(
        without_elapsed(&again) == without_elapsed(&out),
        "同计划两次逐位一致（排除耗时）",
    );

    // 计划自检与严格键：拼错键 / 缺必给字段 / 越界数值 / 阶段与校正不搭
    let err_of = |path: &Path| -> String {
        run(&["deck", "--plan", &path.display().to_string()])
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap_or_default()
    };
    let bad_key = write_plan("bad_key.json", r#"{"stage": "compose", "seed": 1, "ruNs": 10}"#)?;
    checker.check(
        err_of(&bad_key).contains("unknown field"),
        &format!("拼错的键必须被看见: {}", err_of(&bad_key)),
    );
    let no_seed = write_plan("no_seed.json", r#"{"stage": "compose"}"#)?;
    checker.check(
        !err_of(&no_seed).is_empty(),
        &format!("缺 seed 应报错（计划自持可复现）: {}", err_of(&no_seed)),
    );
    let bad_runs = write_plan("bad_runs.json", r#"{"stage": "scan", "seed": 1, "runs": 0}"#)?;
    checker.check(
        err_of(&bad_runs).contains("每臂局数必须"),
        &format!("零局数由计划自检拦下: {}", err_of(&bad_runs)),
    );
    let bad_correct = write_plan(
        "bad_correct.json",
        r#"{"stage": "compose", "seed": 1, "correct": "holm"}"#,
    )?;
    checker.check(
        err_of(&bad_correct).contains("--correct"),
        &format!("compose 阶段的校正口径应报错: {}", err_of(&bad_correct)),
    );
    // cardtest 不接计划（计划只给组卡实验）
    let plan_arg = scan_plan.display().to_string();
    let mut argv_full: Vec<String> = ["cardtest", "--plan", plan_arg.as_str()]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    argv_full.push("--data-root".to_string());
    argv_full.push(root.display().to_string());
    let err = cli::run_from_args_with_host_dir(&argv(&argv_full), &mut Vec::new(), &host)
        .err()
        .map(|e| format!("{e:#}"))
        .unwrap_or_default();
    checker.check(err.contains("--plan"), &format!("cardtest 不认识 --plan: {err}"));

    checker.finish()
}

/// 从机读行里取一个字段值（值内不得含空白，故按空白切分即可）
fn field(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    text.split_whitespace()
        .find_map(|token| token.strip_prefix(&prefix).map(ToString::to_string))
}

/// 去掉机器耗时字段——「同参数两次逐位一致」的判据不含耗时（与 perf 对照纪律一致）
fn without_elapsed(text: &str) -> String {
    text.lines()
        .map(|line| {
            line.split_whitespace()
                .filter(|token| !token.starts_with("elapsed_ms="))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn test_deck_tool() -> Result<()> {
    let host = host_dir("tools-deck-host")?;
    setup(&host)?;
    let root = data_root()?;
    let mut checker = Checker::new("umaai-tools 组卡工具");

    // 入口调用（与命令行同一条链；宿主目录注入，不随运行目录漂移）
    let run = |rest: &[&str]| -> Result<String> {
        let mut argv_full: Vec<String> = rest.iter().map(|s| (*s).to_string()).collect();
        argv_full.push("--data-root".to_string());
        argv_full.push(root.display().to_string());
        let mut out = Vec::new();
        cli::run_from_args_with_host_dir(&argv(&argv_full), &mut out, &host)?;
        String::from_utf8(out).context("输出应为 UTF-8")
    };

    // 构成枚举：默认口径（5 张训练卡 / 排除 30257 → 池 70 张 / 126 种构成）
    let compose = run(&["deck", "--stage", "compose", "--quiet"])?;
    println!("compose 机读输出 = {}", compose.trim_end());
    checker.check(compose.starts_with("DECK_COMPOSE "), "compose 出 DECK_COMPOSE 机读行");
    checker.check(field(&compose, "pool").as_deref() == Some("70"), "默认排除后池为 70 张");
    checker.check(
        field(&compose, "training_slots").as_deref() == Some("5"),
        "默认训练卡槽位为 5",
    );
    checker.check(
        field(&compose, "compositions").as_deref() == Some("126"),
        "五类合计 5 张的构成数为 126",
    );

    // 排除名单可配置：清空后池回到 71 张
    let no_exclude = run(&["deck", "--stage", "compose", "--no-exclude", "--quiet"])?;
    checker.check(
        field(&no_exclude, "pool").as_deref() == Some("71"),
        "--no-exclude 清空排除名单",
    );
    let two_out = run(&["deck", "--stage", "compose", "--exclude", "30257,30301", "--quiet"])?;
    checker.check(
        field(&two_out, "pool").as_deref() == Some("69"),
        "--exclude 覆盖默认名单（两张）",
    );

    // 构成约束可参数化：旧仓 ≤3 口径 101 种；必带卡 + 种类 ≥4 口径 21 种
    let capped = run(&["deck", "--stage", "compose", "--max-per-type", "3", "--quiet"])?;
    checker.check(
        field(&capped, "compositions").as_deref() == Some("101"),
        "单类型 ≤3 时构成数为 101（旧仓口径）",
    );
    let scene = run(&[
        "deck",
        "--stage",
        "compose",
        "--fixed",
        "5=303054",
        "--min-types",
        "4",
        "--quiet",
    ])?;
    checker.check(
        field(&scene, "slots").as_deref() == Some("6") && field(&scene, "training_slots").as_deref() == Some("5"),
        "必带卡占一槽、训练槽位减一",
    );
    checker.check(
        field(&scene, "compositions").as_deref() == Some("21"),
        "种类 ≥4 的构成数为 21（旧仓玩家真实 build 口径）",
    );

    // 实测扫描：跑小样本，检查机读行结构与自证项
    let scan = run(&["deck", "--stage", "scan", "--runs", "40", "--quiet"])?;
    println!("scan 机读输出 =\n{}", scan.trim_end());
    let scan_line = scan.lines().next().unwrap_or_default();
    checker.check(scan_line.starts_with("DECK_SCAN "), "scan 首行是 DECK_SCAN");
    checker.check(
        field(scan_line, "runs").as_deref() == Some("40"),
        "扫描每臂局数与入参一致",
    );
    let boards: Vec<&str> = scan.lines().filter(|l| l.starts_with("DECK_TYPE ")).collect();
    checker.check(!boards.is_empty(), "至少出一个类型榜");
    checker.check(
        boards.iter().all(|b| {
            field(b, "ref_rank").is_some_and(|v| v.parse::<usize>().is_ok_and(|n| n >= 1))
                && field(b, "d_ci95").is_some()
        }),
        "每个类型榜都带参考卡名次与置信半径",
    );
    // 参考卡组自身必须给出零差分（内核自证：管线错位时 scan 直接报错）
    checker.check(
        run(&["deck", "--stage", "scan", "--runs", "10", "--quiet"]).is_ok(),
        "参考卡自证（零差分）通过",
    );

    // 同参数两次必须逐位一致（工具本身要可复现；耗时字段不计入判据）
    let again = run(&["deck", "--stage", "scan", "--runs", "40", "--quiet"])?;
    checker.check(
        without_elapsed(&again) == without_elapsed(&scan),
        "同参数两次扫描逐位一致（排除耗时）",
    );

    // 参考卡组口径守卫：过短点名报错（不落到引擎越界 panic）、被排除的参考卡点名报错
    let err_of = |rest: &[&str]| -> String { run(rest).err().map(|err| format!("{err:#}")).unwrap_or_default() };
    let short = err_of(&[
        "deck",
        "--stage",
        "scan",
        "--runs",
        "2",
        "--seed-deck",
        "303124,303174,303044",
    ]);
    checker.check(
        short.contains("长度不匹配"),
        &format!("过短参考卡组要点名长度不匹配: {short}"),
    );
    let missing = err_of(&["deck", "--stage", "scan", "--runs", "2", "--exclude", "30306"]);
    checker.check(
        missing.contains("不在卡池内"),
        &format!("被排除的参考卡要点名报错: {missing}"),
    );
    let bad_fixed = err_of(&["deck", "--stage", "scan", "--runs", "2", "--fixed", "0=999994"]);
    checker.check(
        bad_fixed.contains("无法解析"),
        &format!("不存在的必带卡要点名报错: {bad_fixed}"),
    );
    let bad_rank = err_of(&["deck", "--stage", "scan", "--runs", "2", "--fixed", "0=303129"]);
    checker.check(
        bad_rank.contains("突破档非法"),
        &format!("越界突破档要点名报错: {bad_rank}"),
    );

    // 必带卡拼进参考卡组（不再静默失效）
    let with_fixed = run(&[
        "deck", "--stage", "scan", "--runs", "10", "--fixed", "5=303054", "--quiet",
    ])?;
    let fixed_line = with_fixed.lines().next().unwrap_or_default();
    checker.check(
        field(fixed_line, "reference").is_some_and(|v| v.ends_with(",303054")),
        "必带卡拼进参考卡组末槽",
    );
    // 与参考卡组其他槽位同卡的候选被跳过（那是重复卡组）
    checker.check(
        field(scan_line, "skipped_duplicates").as_deref() == Some("2"),
        "重复候选被跳过并计数（默认参考卡组速2智2 各一张）",
    );
    checker.check(
        field(scan_line, "skipped_types").as_deref() == Some("2,3"),
        "未出榜类型被点名（默认参考卡组不含力 / 根）",
    );

    // 多重比较校正：缺省 none 不出校正列；holm 出校正 CI / 校正 p / 越过零，且族为该类型榜
    checker.check(
        field(scan_line, "correct").as_deref() == Some("none"),
        "缺省扫描口径不校正",
    );
    checker.check(
        !scan.contains("d_ci_adj="),
        "不校正时 DECK_TYPE 不带校正字段（默认报告口径不变）",
    );
    let holm = run(&[
        "deck",
        "--stage",
        "scan",
        "--runs",
        "40",
        "--correct",
        "holm",
        "--quiet",
    ])?;
    let holm_line = holm.lines().next().unwrap_or_default();
    checker.check(
        field(holm_line, "correct").as_deref() == Some("holm"),
        "机读行点名校正口径",
    );
    let holm_types: Vec<&str> = holm.lines().filter(|l| l.starts_with("DECK_TYPE ")).collect();
    checker.check(
        !holm_types.is_empty()
            && holm_types
                .iter()
                .all(|l| field(l, "d_ci_adj").is_some() && field(l, "p_adj").is_some() && field(l, "beyond").is_some()),
        "校正后每个类型榜都带校正 CI / 校正 p / 越过零",
    );
    let holm_best = holm_types.first().copied().unwrap_or_default();
    let holm_ci: f64 = field(holm_best, "d_ci_adj")
        .and_then(|v| v.parse().ok())
        .unwrap_or(f64::NAN);
    let raw_ci: f64 = field(holm_best, "d_ci95")
        .and_then(|v| v.parse().ok())
        .unwrap_or(f64::NAN);
    checker.check(
        holm_ci >= raw_ci - 1e-9,
        &format!("Holm 校正后的置信半径不小于未校正半径: {holm_ci} vs {raw_ci}"),
    );

    // 排序阶段：两级排序 + 独立验证种子
    let rank = run(&[
        "deck",
        "--stage",
        "rank",
        "--prune",
        "static",
        "--runs",
        "20",
        "--screen-runs",
        "5",
        "--fine-count",
        "3",
        "--holdout-count",
        "2",
        "--quiet",
    ])?;
    println!("rank 机读输出 =\n{}", rank.trim_end());
    let rank_line = rank.lines().next().unwrap_or_default();
    checker.check(rank_line.starts_with("DECK_RANK "), "rank 首行是 DECK_RANK");
    checker.check(
        field(rank_line, "candidates").as_deref() == Some("126"),
        "候选卡组数等于构成数（一种构成一副）",
    );
    checker.check(
        field(rank_line, "seed") != field(rank_line, "holdout_seed"),
        "验证种子缺省独立于选择集",
    );
    let tops: Vec<&str> = rank.lines().filter(|l| l.starts_with("DECK_TOP ")).collect();
    checker.check(tops.len() == 3, "机读行附带前三名明细");
    checker.check(
        tops.iter().all(|l| field(l, "tier").as_deref() == Some("精评")),
        "前三名都进过精评（两级排序不可跨层混排）",
    );
    checker.check(
        tops.iter()
            .filter(|l| field(l, "d_holdout").as_deref() != Some("-"))
            .count()
            == 2,
        "验证只覆盖前 holdout-count 名",
    );
    let rank_again = run(&[
        "deck",
        "--stage",
        "rank",
        "--prune",
        "static",
        "--runs",
        "20",
        "--screen-runs",
        "5",
        "--fine-count",
        "3",
        "--holdout-count",
        "2",
        "--quiet",
    ])?;
    checker.check(
        without_elapsed(&rank_again) == without_elapsed(&rank),
        "同参数两次排序逐位一致（排除耗时）",
    );
    // measured 剪枝：参考卡组里没有的类型（力 / 根）回退静态序并点名，不再静默跳过构成
    let measured = run(&[
        "deck",
        "--stage",
        "rank",
        "--prune",
        "measured",
        "--runs",
        "10",
        "--screen-runs",
        "5",
        "--fine-count",
        "2",
        "--holdout-count",
        "1",
        "--quiet",
    ])?;
    let measured_line = measured.lines().next().unwrap_or_default();
    println!("measured 剪枝机读输出 = {measured_line}");
    checker.check(
        field(measured_line, "fallback").as_deref() == Some("2,3"),
        "缺榜类型回退静态序并点名",
    );
    checker.check(
        field(measured_line, "skipped").as_deref() == Some("0"),
        "回退后不再有装配不出的构成",
    );
    checker.check(
        field(measured_line, "prune").as_deref() == Some("measured"),
        "剪枝口径按入参生效",
    );

    // 验证种子必须独立于选择集：相同即拒绝
    let same_seed = err_of(&[
        "deck",
        "--stage",
        "rank",
        "--runs",
        "5",
        "--screen-runs",
        "2",
        "--seed",
        "9",
        "--holdout-seed",
        "9",
    ]);
    checker.check(
        same_seed.contains("独立"),
        &format!("验证种子与选择集相同应拒绝: {same_seed}"),
    );

    // 参数自检：互斥、未开工阶段、陌生选项、空段、非法观测轴、缺等号、零候选
    for bad in [
        vec!["deck", "--no-exclude", "--exclude", "1"],
        vec!["deck", "--stage", "nope"],
        vec!["deck", "--prune", "nope"],
        vec!["deck", "--screen-runs", "0"],
        vec!["deck", "--fine-count", "0"],
        vec!["cardtest", "--base", "303124,303174,303044,303064,302894"],
        vec!["deck", "--plan", "x"],
        vec!["deck", "--exclude", ""],
        vec!["deck", "--metric", "nope"],
        vec!["deck", "--fixed", "5"],
        vec!["deck", "--top-k", "0"],
        vec!["bench", "--stage", "scan"],
        vec!["compare", "--metric", "pt"],
    ] {
        let argv_full: Vec<String> = bad.iter().map(|s| (*s).to_string()).collect();
        let err = cli::run_from_args_with_host_dir(&argv(&argv_full), &mut Vec::new(), &host)
            .err()
            .map(|e| format!("{e:#}"))
            .unwrap_or_default();
        checker.check(!err.is_empty(), &format!("{bad:?} 应报错: {err}"));
        println!("  {bad:?} -> {}", err.lines().next().unwrap_or_default());
    }

    checker.finish()
}
