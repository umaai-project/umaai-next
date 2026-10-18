//! L4 baseline：逐份比对基线文件与实测
//!
//! 判定壳只做「发现文件 → 跑 → 比对 → 聚合」，模型 / 跑法 / 读写在 `umaai-tools::baseline`；
//! 失败经 `Checker` 聚合并返回 `Err`（自动判定，不用 `assert`）。
//!
//! 重录（跑法不变，只刷新期望值与元信息）：
//!
//! ```text
//! cargo run --release -p umaai-tools --bin umaai-tools -- baseline --update
//! ```
//!
//! 基线守的是**模拟数值位移**：任何改动让同种子整局数值或搜索分布位移，本用例即变红，
//! 重录前应先在 `docs/changelog.md` 记一行改了什么。

use anyhow::{Context, Result, anyhow};
use umaai_core::Data;
use umaai_core::testing::workspace_root;
use umaai_tests::Checker;
use umaai_tools::baseline::{self, Expect};

/// 基线目录（相对 workspace 根）
const DIR: &str = "crates/umaai-tests/baselines";
/// 数据根（相对 workspace 根）
const DATA_DIR: &str = "gamedata";
/// 基线份数下限：整局 / 种子扫描 / 固定根搜索各一份
const MIN_CASES: usize = 3;

#[test]
fn baseline_matches_recorded() -> Result<()> {
    let root = workspace_root()?;
    Data::init(root.join(DATA_DIR)).context("载入数据根失败")?;
    let dir = root.join(DIR);
    let files = baseline::list_cases(&dir).context("基线目录不可用")?;

    let mut checker = Checker::new("baseline 基线判定");
    let mut drift_guard = false;
    for path in &files {
        let case = baseline::load_case(path)?;
        let facts = baseline::run_case(&case)?;
        let diffs = baseline::compare(&facts, &case.expect);
        for d in &diffs {
            println!("      {}: {}", d.key, d.detail);
        }
        checker.check(
            diffs.is_empty(),
            &format!(
                "{} 与基线一致（{} 项观测，记录于 {}）",
                case.case,
                case.expect.len(),
                case.meta.updated
            ),
        );

        // 判定不许恒真：首份基线人为改动一项期望，必须报出不一致（只比对、不写文件）
        if !drift_guard {
            drift_guard = true;
            let broken = break_one(&case)?;
            let caught = baseline::compare(&facts, &broken.expect);
            checker.check(
                !caught.is_empty(),
                &format!("{} 判定可红（改动一项期望即报不一致）", case.case),
            );
        }
    }
    checker.check(files.len() >= MIN_CASES, &format!("基线份数不少于 {MIN_CASES}"));
    checker.finish()
}

/// 复制一份基线，把**数值**期望整体加一（键序不变，便于稳定命中同一项）
fn break_one(case: &baseline::Case) -> Result<baseline::Case> {
    let mut broken = case.clone();
    let key = broken
        .expect
        .iter()
        .find(|(_, v)| v.value().is_number())
        .map(|(k, _)| k.clone())
        .ok_or_else(|| anyhow!("基线 {} 没有数值观测项，无法守门", case.case))?;
    let value = broken.expect[&key].value().as_f64().unwrap_or(0.0);
    broken
        .expect
        .insert(key.clone(), Expect::Exact(serde_json::json!(value + 1.0)));
    Ok(broken)
}
