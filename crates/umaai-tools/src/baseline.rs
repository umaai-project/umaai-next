//! 基线：模型 / 跑法 / 比对 / 读写
//!
//! 一基线一 JSON：`input`（**冻结的自持输入**，不读配置层，故宿主覆盖层不会污染基线）
//! 与 `expect`（逐项期望值 + 容差）分离，`meta` 记生成元信息。判定壳在
//! `umaai-tests/tests/baseline.rs`，重录走 `umaai-tools baseline --update`。
//!
//! 基线守的是**模拟数值位移**（终局评分 / 五维 / 搜索分布）：同种子整局结果与搜索分布
//! 每变动一次整份位移，就靠这里逐项比对。编排与呈现不在此守门（各有端到端用例）。
//!
//! 生成元信息只记程序与数据的**版本号与日期**，不记任何路径——`DataSource` 的来源标识
//! 是路径字符串，落进基线文件就等于泄露宿主目录结构。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use umaai_core::Data;
use umaai_core::data::manifest::SCHEMA_VERSION as DATA_SCHEMA_VERSION;
use umasim::game::base::basic::BasicGame;
use umasim::game::{Game, InheritInfo};
use umasim::search::SearchConfig;

use crate::setup;

/// 基线文件格式版本（结构变动即 +1，旧文件按版本拒绝）
pub const SCHEMA_VERSION: u32 = 1;

/// 基线文件后缀
pub const FILE_SUFFIX: &str = "json";

/// 自动记录非整数数值项时给的默认容差（整数项逐位比对，容差 0）
pub const DEFAULT_TOL: f64 = 1e-6;

/// 观测事实集合：键 → JSON 值
pub type Facts = BTreeMap<String, Value>;

/// 跑法种类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// 整局：固定种子跑满一局，记录终局事实
    FullRun,
    /// 种子扫描：`seed..seed+runs` 连续若干局，记录逐局评分与合计
    SeedSweep,
    /// 固定根搜索：推进到首个训练决策点后跑一次搜索，记录候选与预算
    SearchRoot,
}

/// 冻结的跑法输入（自持：不读配置层，故覆盖层改不动基线）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// 跑法种类
    pub kind: Kind,
    /// 马娘 ID
    pub uma: u32,
    /// 卡组（支援卡 ID，不得重复）
    pub cards: Vec<u32>,
    /// 种马蓝因子个数 `[速, 耐, 力, 根, 智]`
    pub blue_count: [i32; 5],
    /// 种马额外属性 `[速, 耐, 力, 根, 智, 技能点]`
    pub extra_count: [i32; 6],
    /// 比赛等级表（72 项；与配置层 `standard` 预设同值时可直接抄）
    pub race_grades: Vec<i32>,
    /// 基础种子（种子扫描时第 i 局用 `seed + i`）
    pub seed: u64,
    /// 扫描局数（仅 `seed_sweep` 用；缺省 1）
    #[serde(default = "one")]
    pub runs: u32,
    /// 搜索参数（仅 `search_root` 用；缺省即报错）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchConfig>,
}

/// 缺省局数
fn one() -> u32 {
    1
}

impl Input {
    /// 按输入构造一局（基线不使用配置层，参数全部来自文件）
    ///
    /// 输入的种子同时作为**规则主种子**：规则流按 `(master, 回合)` 派生，基线因此把
    /// "用哪个主种子" 也一并冻结在文件里。
    fn newgame(&self) -> Result<BasicGame> {
        let mut game = BasicGame::newgame(
            self.uma,
            &self.cards,
            InheritInfo {
                blue_count: self.blue_count,
                extra_count: self.extra_count,
            },
            &self.race_grades,
        )
        .with_context(|| format!("基线输入构造局面失败（马娘 {}）", self.uma))?;
        game.set_rule_master(self.seed);
        Ok(game)
    }

    /// 跑法自检：局数、搜索参数的必需性
    fn validate(&self) -> Result<()> {
        ensure!(!self.cards.is_empty(), "卡组不得为空");
        ensure!(!self.race_grades.is_empty(), "比赛等级表不得为空");
        match self.kind {
            Kind::FullRun => ensure!(self.runs == 1, "full_run 的 runs 必须为 1"),
            Kind::SeedSweep => ensure!(self.runs >= 1, "seed_sweep 的 runs 必须 ≥ 1"),
            Kind::SearchRoot => ensure!(
                self.search.is_some(),
                "search_root 必须给出 search 段（搜索预算与口径随基线一起冻结）"
            ),
        }
        Ok(())
    }
}

/// 生成元信息
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    /// 记录本基线的工具版本
    pub tool_version: String,
    /// 记录时的数据 schema 版本（数据更新后据此判断基线是否需要重录）
    pub data_schema_version: u32,
    /// 最近一次记录日期 `YYYY-MM-DD`
    pub updated: String,
    /// 备注（口径说明，一行）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 单项期望：裸值为精确比对，`{value, tol}` 形式带容差
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Expect {
    /// 带容差的期望（写在前面，否则会被裸值分支吞掉）
    Tolerant {
        /// 期望值
        value: Value,
        /// 允许的绝对偏差
        tol: f64,
    },
    /// 精确期望（容差 0）
    Exact(Value),
}

impl Expect {
    /// 期望值
    #[must_use]
    pub fn value(&self) -> &Value {
        match self {
            Self::Tolerant { value, .. } => value,
            Self::Exact(value) => value,
        }
    }

    /// 容差
    #[must_use]
    pub fn tol(&self) -> f64 {
        match self {
            Self::Tolerant { tol, .. } => *tol,
            Self::Exact(_) => 0.0,
        }
    }
}

/// 一份基线
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// 文件格式版本
    pub schema_version: u32,
    /// 基线名（与文件名主干一致）
    pub case: String,
    /// 生成元信息
    pub meta: Meta,
    /// 冻结的跑法输入
    pub input: Input,
    /// 逐项期望
    pub expect: BTreeMap<String, Expect>,
}

/// 一处不一致
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// 观测项键（数组元素带 `[i]` 后缀）
    pub key: String,
    /// 差异说明
    pub detail: String,
}

/// 跑一份基线，返回观测事实（要求全局静态数据已初始化）
pub fn run_case(case: &Case) -> Result<Facts> {
    ensure!(
        case.schema_version == SCHEMA_VERSION,
        "基线 {} 格式版本 {} 与本程序 {} 不一致（需重录或升级程序）",
        case.case,
        case.schema_version,
        SCHEMA_VERSION
    );
    ensure!(!case.case.trim().is_empty(), "基线名不得为空");
    case.input.validate()?;
    ensure!(
        Data::try_get().is_some(),
        "全局静态数据未初始化：宿主应先调用 umaai_core::Data::init(data_root)"
    );
    match case.input.kind {
        Kind::FullRun => full_run(&case.input),
        Kind::SeedSweep => seed_sweep(&case.input),
        Kind::SearchRoot => search_root(&case.input),
    }
}

/// 整局：跑满一局并取终局事实
fn full_run(input: &Input) -> Result<Facts> {
    let game = setup::run_full(input.newgame()?, input.seed)?;
    final_facts(&game)
}

/// 种子扫描：连续若干局的评分序列（守「单颗种子侥幸」之外的宽面位移）
fn seed_sweep(input: &Input) -> Result<Facts> {
    let mut scores = Vec::with_capacity(input.runs as usize);
    for i in 0..input.runs {
        let game = setup::run_full(input.newgame()?, input.seed + u64::from(i))?;
        scores.push(game.uma().calc_score()?);
    }
    let sum: i64 = scores.iter().map(|s| i64::from(*s)).sum();
    let min = scores.iter().copied().min().unwrap_or(0);
    let max = scores.iter().copied().max().unwrap_or(0);
    Ok(BTreeMap::from([
        ("scores".to_string(), json!(scores)),
        ("score_sum".to_string(), json!(sum)),
        ("score_min".to_string(), json!(min)),
        ("score_max".to_string(), json!(max)),
    ]))
}

/// 固定根搜索：候选预算与均分
fn search_root(input: &Input) -> Result<Facts> {
    let search = input.search.as_ref().context("search_root 缺少 search 段")?;
    let (game, actions) = setup::decision_root(input.newgame()?, input.seed)?;
    let rep = crate::probe::run_search(&game, &actions, search, input.seed)?;
    Ok(BTreeMap::from([
        ("turn".to_string(), json!(rep.turn)),
        ("stage_key".to_string(), json!(rep.stage_key)),
        ("candidates".to_string(), json!(rep.candidates)),
        ("planned".to_string(), json!(rep.planned)),
        ("succeeded".to_string(), json!(rep.succeeded)),
        ("best_idx".to_string(), json!(rep.best_idx)),
        ("best_action".to_string(), json!(rep.best_action)),
        ("means".to_string(), json!(rep.means)),
        ("root_score".to_string(), json!(rep.root_score)),
    ]))
}

/// 终局事实
fn final_facts(game: &BasicGame) -> Result<Facts> {
    let uma = game.uma();
    Ok(BTreeMap::from([
        ("uma_id".to_string(), json!(uma.uma_id)),
        ("score".to_string(), json!(uma.calc_score()?)),
        ("pt".to_string(), json!(uma.total_pt()?)),
        ("skill_score".to_string(), json!(uma.skill_score)),
        ("five_status".to_string(), json!(uma.five_status.to_vec())),
        ("turn".to_string(), json!(game.turn())),
    ]))
}

/// 比对观测事实与期望，返回全部不一致项（空即通过）
pub fn compare(facts: &Facts, expect: &BTreeMap<String, Expect>) -> Vec<Difference> {
    let mut diffs = Vec::new();
    for (key, want) in expect {
        match facts.get(key) {
            None => diffs.push(Difference {
                key: key.clone(),
                detail: "观测项缺失（跑法与程序版本不匹配？）".to_string(),
            }),
            Some(got) => diff_value(key, got, want.value(), want.tol(), &mut diffs),
        }
    }
    for key in facts.keys() {
        if !expect.contains_key(key) {
            diffs.push(Difference {
                key: key.clone(),
                detail: "多出未记录的观测项（重录以固化）".to_string(),
            });
        }
    }
    diffs
}

/// 逐类型比对：数值按容差、数组逐元素、其余精确
fn diff_value(path: &str, got: &Value, want: &Value, tol: f64, out: &mut Vec<Difference>) {
    match (got, want) {
        (Value::Number(g), Value::Number(w)) => {
            let (g, w) = (g.as_f64().unwrap_or(f64::NAN), w.as_f64().unwrap_or(f64::NAN));
            if !(g - w).abs().le(&tol) {
                out.push(Difference {
                    key: path.to_string(),
                    detail: format!("期望 {w}（容差 {tol}），实测 {g}"),
                });
            }
        }
        (Value::Array(g), Value::Array(w)) => {
            if g.len() != w.len() {
                out.push(Difference {
                    key: path.to_string(),
                    detail: format!("长度不一致：期望 {}，实测 {}", w.len(), g.len()),
                });
                return;
            }
            for (i, (g, w)) in g.iter().zip(w.iter()).enumerate() {
                diff_value(&format!("{path}[{i}]"), g, w, tol, out);
            }
        }
        _ => {
            if got != want {
                out.push(Difference {
                    key: path.to_string(),
                    detail: format!("期望 {want}，实测 {got}"),
                });
            }
        }
    }
}

/// 用观测事实重录期望与元信息（保留输入、名字与备注）
pub fn record(case: &Case) -> Result<Case> {
    let facts = run_case(case)?;
    let expect = facts.iter().map(|(k, v)| (k.clone(), expect_of(v))).collect();
    let mut meta = case.meta.clone();
    meta.tool_version = env!("CARGO_PKG_VERSION").to_string();
    meta.data_schema_version = DATA_SCHEMA_VERSION;
    meta.updated = today();
    Ok(Case {
        schema_version: SCHEMA_VERSION,
        case: case.case.clone(),
        meta,
        input: case.input.clone(),
        expect,
    })
}

/// 由观测值造期望项：非整数数值（含数组里的非整数元素）自动带默认容差，整数与文本精确比对
fn expect_of(value: &Value) -> Expect {
    if needs_tol(value) {
        Expect::Tolerant {
            value: value.clone(),
            tol: DEFAULT_TOL,
        }
    } else {
        Expect::Exact(value.clone())
    }
}

/// 是否含非整数数值（整数逐位、浮点带容差）
fn needs_tol(value: &Value) -> bool {
    match value {
        Value::Number(n) => n.as_f64().is_some_and(|f| f.fract() != 0.0),
        Value::Array(items) => items.iter().any(needs_tol),
        _ => false,
    }
}

/// 读取一份基线
pub fn load_case(path: &Path) -> Result<Case> {
    let text = std::fs::read_to_string(path).with_context(|| format!("读取基线失败: {}", path.display()))?;
    let case: Case = serde_json::from_str(&text).with_context(|| format!("解析基线失败: {}", path.display()))?;
    ensure!(
        case.schema_version == SCHEMA_VERSION,
        "基线 {} 的格式版本为 {}，本程序支持 {}",
        path.display(),
        case.schema_version,
        SCHEMA_VERSION
    );
    Ok(case)
}

/// 写回一份基线（键序稳定：事实键按字典序、尾随换行）
pub fn save_case(path: &Path, case: &Case) -> Result<()> {
    let mut text = serde_json::to_string_pretty(case).context("序列化基线失败")?;
    text.push('\n');
    std::fs::write(path, text).with_context(|| format!("写入基线失败: {}", path.display()))?;
    Ok(())
}

/// 列出目录下的基线文件（按路径排序；不存在即报错，不静默当空）
pub fn list_cases(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = std::fs::read_dir(dir).with_context(|| format!("基线目录不可读: {}", dir.display()))?;
    for entry in entries {
        let path = entry
            .with_context(|| format!("遍历基线目录失败: {}", dir.display()))?
            .path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == FILE_SUFFIX) {
            files.push(path);
        }
    }
    files.sort();
    ensure!(
        !files.is_empty(),
        "基线目录内没有 .{FILE_SUFFIX} 文件: {}",
        dir.display()
    );
    Ok(files)
}

/// 今天的日期（UTC，`YYYY-MM-DD`）；只为元信息记一笔，不参与任何判据
#[must_use]
pub fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(i64::try_from(secs / 86_400).unwrap_or(0));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Unix 天数 → `(年, 月, 日)`（Howard Hinnant 的 civil_from_days，纪元 1970-01-01 = 0）
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = u64::try_from(z - era * 146_097).unwrap_or(0);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = i64::try_from(yoe).unwrap_or(0) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(0);
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, u32::try_from(m).unwrap_or(0), d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一份最小可用输入
    fn input() -> Input {
        Input {
            kind: Kind::FullRun,
            uma: 100101,
            cards: vec![1, 2],
            blue_count: [15, 3, 0, 0, 0],
            extra_count: [0, 30, 0, 0, 30, 30],
            race_grades: vec![0; 72],
            seed: 42,
            runs: 1,
            search: None,
        }
    }

    /// 层1：日期换算对齐已知纪元（含闰年与世纪边界）
    #[test]
    fn test_civil_from_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(1), (1970, 1, 2));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29), "2000 是闰年");
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_000), (2024, 10, 4));
    }

    /// 层1：今天的日期形如 `YYYY-MM-DD`
    #[test]
    fn test_today_shape() {
        let d = today();
        assert_eq!(d.len(), 10, "日期形如 YYYY-MM-DD: {d}");
        assert_eq!(d.matches('-').count(), 2);
    }

    /// 层1：整数与文本走精确比对，非整数与含浮点元素的数组自动带容差
    #[test]
    fn test_expect_of() {
        assert_eq!(expect_of(&json!(7)), Expect::Exact(json!(7)));
        assert_eq!(expect_of(&json!("速度")), Expect::Exact(json!("速度")));
        assert_eq!(expect_of(&json!([1, 2, 3])), Expect::Exact(json!([1, 2, 3])));

        let t = expect_of(&json!([1.5, 2.0]));
        assert_eq!(t.tol(), DEFAULT_TOL);
        assert_eq!(t.value(), &json!([1.5, 2.0]));
    }

    /// 层1：比对按容差与数组逐元素展开，缺项与多项都报
    #[test]
    fn test_compare_paths() {
        let facts = BTreeMap::from([
            ("score".to_string(), json!(100)),
            ("five".to_string(), json!([1, 2, 3])),
            ("extra".to_string(), json!(1)),
        ]);
        let expect = BTreeMap::from([
            ("score".to_string(), Expect::Exact(json!(100))),
            ("five".to_string(), Expect::Exact(json!([1, 9, 3]))),
            ("missing".to_string(), Expect::Exact(json!(1))),
        ]);
        let diffs = compare(&facts, &expect);
        let keys: Vec<&str> = diffs.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(keys, vec!["five[1]", "missing", "extra"]);
    }

    /// 层1：容差内的浮点差异不算不一致，超差才算
    #[test]
    fn test_compare_tolerance() {
        let facts = BTreeMap::from([("m".to_string(), json!(1.0 + 1e-9))]);
        let ok = BTreeMap::from([("m".to_string(), Expect::Exact(json!(1.0)))]);
        assert_eq!(compare(&facts, &ok).len(), 1, "零容差下浮点也必须逐位一致");

        let tol = BTreeMap::from([("m".to_string(), Expect::Tolerant { value: json!(1.0), tol: 1e-6 })]);
        assert!(compare(&facts, &tol).is_empty());
    }

    /// 层1：数组长度不一致直接报长度，不再逐元素展开
    #[test]
    fn test_compare_array_len() {
        let facts = BTreeMap::from([("a".to_string(), json!([1, 2]))]);
        let expect = BTreeMap::from([("a".to_string(), Expect::Exact(json!([1, 2, 3])))]);
        let diffs = compare(&facts, &expect);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].detail.contains("长度"), "{}", diffs[0].detail);
    }

    /// 层1：裸值与带容差两种写法都能反序列化，且带容差分支优先
    #[test]
    fn test_expect_serde() {
        let exact: Expect = serde_json::from_str("7").expect("裸值");
        assert_eq!(exact.tol(), 0.0);
        let tolerant: Expect = serde_json::from_str(r#"{"value": 7, "tol": 0.5}"#).expect("带容差");
        assert_eq!(tolerant.tol(), 0.5);
        assert_eq!(tolerant.value(), &json!(7));
    }

    /// 层1：跑法自检拦住缺搜索段的固定根用例与非 1 局数的整局用例
    #[test]
    fn test_input_validate() {
        assert!(input().validate().is_ok());

        let mut bad = input();
        bad.kind = Kind::SearchRoot;
        assert!(bad.validate().is_err(), "固定根搜索必须给出 search 段");

        let mut bad = input();
        bad.runs = 3;
        assert!(bad.validate().is_err(), "整局用例的 runs 必须为 1");

        let mut bad = input();
        bad.cards.clear();
        assert!(bad.validate().is_err(), "卡组不得为空");
    }

    /// 层1：格式版本不符即拒绝
    #[test]
    fn test_run_case_rejects_old_schema() {
        let case = Case {
            schema_version: SCHEMA_VERSION + 1,
            case: "x".to_string(),
            meta: Meta {
                tool_version: "0".to_string(),
                data_schema_version: 1,
                updated: "1970-01-01".to_string(),
                note: None,
            },
            input: input(),
            expect: BTreeMap::new(),
        };
        let err = run_case(&case).expect_err("版本不符应报错");
        let msg = format!("{err:#}");
        assert!(msg.contains("格式版本"), "错误文案应指明格式版本: {msg}");
    }
}
