//! 引擎配置：默认层 + 覆盖层
//!
//! 两份文件**同名不同位**，都叫 `ai_config.toml`：
//!
//! - **默认层**：数据根内，字段**必须齐全**，Rust 侧不为同名字段提供默认值（单一真源）；
//!   缺字段即报错并指明字段路径，避免「代码缺省 / 文件缺省」两份真相。
//! - **覆盖层**：宿主目录（桌面入口取当前目录）内，与默认层同构、字段全 `Option`
//!   （`None` = 沿用默认层），段缺失即整段沿用，不会把默认值清空；**存在才合并**。
//!
//! 配置与程序同版本发布，故对**未知键严格报错**（拼错键必须被看见）；对外协议
//! （帧解析）另按宽容口径，二者不共用纪律。
//!
//! 剧本专属配置不进本层：剧本仓自持自己的配置段，引擎只保留剧本无关的通用段。
//!
//! 非路径宿主（Web / Android）把默认层与覆盖层的 TOML 文本喂 [`Config::parse`]：
//! 静态表的多来源由 `umaai_core::DataSource` 承担，**配置不纳入该来源**——TOML 文本与
//! JSON 表是两种形态，混进一个 trait 只会让两边都变重。

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use umasim::{game::InheritInfo, search::SearchConfig};

/// 配置文件名：数据根内是**默认层**（字段齐全、单一真源），宿主目录内是**覆盖层**（字段可选）
pub const CONFIG_FILE: &str = "ai_config.toml";

/// 引擎配置（只含剧本无关的通用段）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// 一局构造与运行参数
    pub sim: SimConfig,
    /// 搜索与算法参数
    pub search: SearchConfig,
    /// 在线记录参数
    pub record: RecordConfig,
    /// 输出与日志参数（由入口层消费，引擎自己不打印）
    pub output: OutputConfig,
    /// 具名预设表，按字段里的名字取用
    pub presets: Presets,
}

/// 一局构造与运行参数
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimConfig {
    /// 剧本选择器（不透明字符串，值域由剧本仓与入口层约定，引擎不解释）
    pub scenario: String,
    /// 训练员选择器（同上）
    pub trainer: String,
    /// 马娘 ID
    pub uma: u32,
    /// 卡组（支援卡 ID，长度不固定，不得重复）
    pub cards: Vec<u32>,
    /// 种马蓝因子个数 `[速, 耐, 力, 根, 智]`
    pub blue_count: [i32; 5],
    /// 种马额外属性 `[速, 耐, 力, 根, 智, 技能点]`
    pub extra_count: [i32; 6],
    /// 基础随机种子（第 i 局用 `seed + i`）
    pub seed: u64,
    /// 批量模拟局数（单局入口取第 0 局）
    pub runs: u32,
    /// 比赛等级表预设名（见 [`Presets::race_grades`]）
    pub race_grades: String,
}

/// 在线记录参数
///
/// 引擎只产出记录数据，落盘由上层（`umaai-cli` / `umaai-tools`）实现；本段由上层读取。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordConfig {
    /// 是否开启在线记录
    pub enabled: bool,
    /// 记录根目录（相对路径按宿主工作目录解释）
    pub root: String,
    /// 运气分显示分换算的每回合加成（旧仓沿用 70；仅影响运气分口径，不影响模拟）
    pub luck_turn_bonus: i32,
}

/// 输出与日志参数
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    /// 日志级别（由入口层初始化日志器时消费）
    pub log_level: String,
}

/// 具名预设表集合
///
/// 语义与 bench 的预设卡组一致：**按名取、缺失即报错**，不静默兜底到内置表。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Presets {
    /// 比赛等级表预设：名字 → 「回合 → 等级」数组（等级 0-4）
    pub race_grades: BTreeMap<String, Vec<i32>>,
}

impl Config {
    /// 载入：默认层文件 + 可选覆盖层文件
    pub fn load(base: impl AsRef<Path>, overlay: Option<&Path>) -> Result<Self> {
        let base = base.as_ref();
        let base_text = fs::read_to_string(base).with_context(|| format!("读取默认配置失败: {}", base.display()))?;
        let overlay_text = match overlay {
            Some(path) => {
                Some(fs::read_to_string(path).with_context(|| format!("读取覆盖配置失败: {}", path.display()))?)
            }
            None => None,
        };
        Self::parse(&base_text, overlay_text.as_deref()).with_context(|| format!("默认配置来源: {}", base.display()))
    }

    /// 载入数据根下的默认层（`<data_root>/ai_config.toml`）+ 显式指定的覆盖层文件
    ///
    /// 覆盖层路径给了就必须存在（探针 / 测试要的就是「路径写错立刻报错」）；想要
    /// 「存在才合并」的约定形态用 [`Self::load_with_overlay_file`]。
    pub fn load_from_data_root(data_root: impl AsRef<Path>, overlay: Option<&Path>) -> Result<Self> {
        Self::load(data_root.as_ref().join(CONFIG_FILE), overlay)
    }

    /// 载入：数据根默认层 + **约定覆盖层**（`<dir>/ai_config.toml`，存在才合并）
    ///
    /// 覆盖层与默认层同名，靠目录区分角色：桌面入口传工作目录。文件缺席不报错——
    /// 首次运行本就没有覆盖层，这正是「只留一个覆盖类配置文件」的默认形态。
    pub fn load_with_overlay_file(data_root: impl AsRef<Path>, dir: impl AsRef<Path>) -> Result<Self> {
        let overlay = dir.as_ref().join(CONFIG_FILE);
        let overlay = overlay.exists().then_some(overlay);
        Self::load_from_data_root(data_root, overlay.as_deref())
    }

    /// 解析 TOML 文本（默认层 + 可选覆盖层文本），供内嵌 / 测试场景复用
    pub fn parse(base_toml: &str, overlay_toml: Option<&str>) -> Result<Self> {
        let mut cfg: Config = toml::from_str(base_toml).context("解析默认配置失败")?;
        if let Some(text) = overlay_toml {
            let overlay: Overlay = toml::from_str(text).context("解析覆盖配置失败")?;
            overlay.apply(&mut cfg);
        }
        cfg.validate()?;
        Ok(cfg)
    }

    /// 按 `sim.race_grades` 的名字取比赛等级表
    pub fn race_grades(&self) -> Result<&[i32]> {
        let name = &self.sim.race_grades;
        match self.presets.race_grades.get(name) {
            Some(grades) => Ok(grades.as_slice()),
            None => {
                let names = self.presets.race_grades.keys().cloned().collect::<Vec<_>>();
                let avail = if names.is_empty() {
                    "无".to_string()
                } else {
                    names.join(", ")
                };
                Err(anyhow!("presets.race_grades 未定义预设 \"{name}\"（可用: {avail}）"))
            }
        }
    }

    /// 构造继承因子
    pub fn inherit(&self) -> InheritInfo {
        InheritInfo {
            blue_count: self.sim.blue_count,
            extra_count: self.sim.extra_count,
        }
    }

    /// 校验：字段合法性 + 预设可解析
    fn validate(&self) -> Result<()> {
        if self.sim.cards.is_empty() {
            bail!("sim.cards 不能为空");
        }
        // 人头与卡组按下标平行，重复卡会让反查静默命中第一张
        let mut seen = std::collections::BTreeSet::new();
        for id in &self.sim.cards {
            if !seen.insert(*id) {
                bail!("sim.cards 含重复支援卡: {id}");
            }
        }
        if self.sim.blue_count.iter().any(|v| *v < 0) || self.sim.extra_count.iter().any(|v| *v < 0) {
            bail!("sim.blue_count / sim.extra_count 不得为负");
        }
        if self.sim.runs == 0 {
            bail!("sim.runs 必须 ≥ 1");
        }
        if self.record.luck_turn_bonus < 0 {
            bail!(
                "record.luck_turn_bonus 不得为负（当前 {}）",
                self.record.luck_turn_bonus
            );
        }
        let grades = self.race_grades()?;
        // 生涯比赛表覆盖回合 0-71，URA 回合 72-77 固定 G1 不进表
        if grades.len() < 72 {
            bail!(
                "比赛等级表 \"{}\" 长度 {} 不足 72（需覆盖回合 0-71）",
                self.sim.race_grades,
                grades.len()
            );
        }
        if let Some(bad) = grades.iter().find(|g| !(0..=4).contains(*g)) {
            bail!("比赛等级表 \"{}\" 含非法等级 {bad}（应为 0-4）", self.sim.race_grades);
        }
        Ok(())
    }
}

/// 覆盖层：与默认层同构、字段全 `Option`（`None` = 沿用默认层）
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Overlay {
    sim: Option<SimOverlay>,
    search: Option<SearchOverlay>,
    record: Option<RecordOverlay>,
    output: Option<OutputOverlay>,
    presets: Option<PresetsOverlay>,
}

impl Overlay {
    fn apply(self, cfg: &mut Config) {
        if let Some(ov) = self.sim {
            ov.apply(&mut cfg.sim);
        }
        if let Some(ov) = self.search {
            ov.apply(&mut cfg.search);
        }
        if let Some(ov) = self.record {
            ov.apply(&mut cfg.record);
        }
        if let Some(ov) = self.output {
            ov.apply(&mut cfg.output);
        }
        // 预设按名字合并：同名覆盖，未提及的名字保留默认层条目
        if let Some(ov) = self.presets {
            cfg.presets.race_grades.extend(ov.race_grades);
        }
    }
}

/// `sim` 段覆盖：字段全 `Option`，类型与 [`SimConfig`] 一一对应
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SimOverlay {
    scenario: Option<String>,
    trainer: Option<String>,
    uma: Option<u32>,
    cards: Option<Vec<u32>>,
    blue_count: Option<[i32; 5]>,
    extra_count: Option<[i32; 6]>,
    seed: Option<u64>,
    runs: Option<u32>,
    race_grades: Option<String>,
}

impl SimOverlay {
    fn apply(self, cfg: &mut SimConfig) {
        if let Some(v) = self.scenario {
            cfg.scenario = v;
        }
        if let Some(v) = self.trainer {
            cfg.trainer = v;
        }
        if let Some(v) = self.uma {
            cfg.uma = v;
        }
        if let Some(v) = self.cards {
            cfg.cards = v;
        }
        if let Some(v) = self.blue_count {
            cfg.blue_count = v;
        }
        if let Some(v) = self.extra_count {
            cfg.extra_count = v;
        }
        if let Some(v) = self.seed {
            cfg.seed = v;
        }
        if let Some(v) = self.runs {
            cfg.runs = v;
        }
        if let Some(v) = self.race_grades {
            cfg.race_grades = v;
        }
    }
}

/// `search` 段覆盖：字段全 `Option`，类型与 [`SearchConfig`] 一一对应
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchOverlay {
    search_n: Option<usize>,
    max_depth: Option<usize>,
    radical_factor_max: Option<f64>,
    use_ucb: Option<bool>,
    search_group_size: Option<usize>,
    search_cpuct: Option<f64>,
    expected_search_stdev: Option<f64>,
    record_ordered_rollouts: Option<bool>,
    pt_favor_rate: Option<f64>,
}

impl SearchOverlay {
    fn apply(self, cfg: &mut SearchConfig) {
        if let Some(v) = self.search_n {
            cfg.search_n = v;
        }
        if let Some(v) = self.max_depth {
            cfg.max_depth = v;
        }
        if let Some(v) = self.radical_factor_max {
            cfg.radical_factor_max = v;
        }
        if let Some(v) = self.use_ucb {
            cfg.use_ucb = v;
        }
        if let Some(v) = self.search_group_size {
            cfg.search_group_size = v;
        }
        if let Some(v) = self.search_cpuct {
            cfg.search_cpuct = v;
        }
        if let Some(v) = self.expected_search_stdev {
            cfg.expected_search_stdev = v;
        }
        if let Some(v) = self.record_ordered_rollouts {
            cfg.record_ordered_rollouts = v;
        }
        if let Some(v) = self.pt_favor_rate {
            cfg.pt_favor_rate = v;
        }
    }
}

/// `record` 段覆盖
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordOverlay {
    enabled: Option<bool>,
    root: Option<String>,
    luck_turn_bonus: Option<i32>,
}

impl RecordOverlay {
    fn apply(self, cfg: &mut RecordConfig) {
        if let Some(v) = self.enabled {
            cfg.enabled = v;
        }
        if let Some(v) = self.root {
            cfg.root = v;
        }
        if let Some(v) = self.luck_turn_bonus {
            cfg.luck_turn_bonus = v;
        }
    }
}

/// `output` 段覆盖
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputOverlay {
    log_level: Option<String>,
}

impl OutputOverlay {
    fn apply(self, cfg: &mut OutputConfig) {
        if let Some(v) = self.log_level {
            cfg.log_level = v;
        }
    }
}

/// 预设段的覆盖层：同名覆盖，未提及的名字保留默认层
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetsOverlay {
    race_grades: BTreeMap<String, Vec<i32>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 72 项零等级表（结构合法的最小预设，仅测解析用）
    fn grades() -> String {
        grades_with_first(0)
    }

    /// 首项为 `first`、其余为 0 的 72 项等级表
    fn grades_with_first(first: i32) -> String {
        let head = first.to_string();
        let mut items = std::iter::repeat_n("0", 71).collect::<Vec<_>>();
        items.insert(0, head.as_str());
        format!("[{}]", items.join(", "))
    }

    /// 完整默认层（字段齐全，无缺省）
    fn base() -> String {
        format!(
            r#"
[sim]
scenario = "basic"
trainer = "random"
uma = 100101
cards = [302424, 302464]
blue_count = [15, 3, 0, 0, 0]
extra_count = [0, 30, 0, 0, 30, 30]
seed = 42
runs = 1
race_grades = "standard"

[search]
search_n = 1024
max_depth = 0
radical_factor_max = 50.0
use_ucb = true
search_group_size = 256
search_cpuct = 1.0
expected_search_stdev = 2200.0
record_ordered_rollouts = false
pt_favor_rate = 2.0

[record]
enabled = false
root = "logs"
luck_turn_bonus = 70

[output]
log_level = "info"

[presets.race_grades]
standard = {}
"#,
            grades()
        )
    }

    /// 层1：默认层解析成功，字段与预设可解析
    #[test]
    fn test_parse_base() -> Result<()> {
        let cfg = Config::parse(&base(), None)?;
        assert_eq!(cfg.sim.uma, 100101);
        assert_eq!(cfg.sim.cards.len(), 2);
        assert_eq!(cfg.search.search_n, 1024);
        assert_eq!(cfg.search.pt_favor_rate, 2.0);
        assert!(!cfg.record.enabled);
        assert_eq!(cfg.race_grades()?.len(), 72);
        assert_eq!(cfg.inherit().blue_count, [15, 3, 0, 0, 0]);
        Ok(())
    }

    /// 层1：缺字段必须报错（不留代码缺省）
    #[test]
    fn test_missing_field_rejected() {
        let bad = base().replace("search_n = 1024\n", "");
        let err = Config::parse(&bad, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("search_n"), "错误应指明缺字段: {msg}");
    }

    /// 层1：覆盖层逐字段生效，未提及字段沿用默认层
    #[test]
    fn test_overlay_partial() -> Result<()> {
        let cfg = Config::parse(&base(), Some("[sim]\numa = 102601\n[search]\npt_favor_rate = 1.0\n"))?;
        assert_eq!(cfg.sim.uma, 102601, "覆盖字段生效");
        assert_eq!(cfg.sim.cards.len(), 2, "未提及的字段保留");
        assert_eq!(cfg.sim.seed, 42);
        assert_eq!(cfg.search.pt_favor_rate, 1.0);
        assert_eq!(cfg.search.search_n, 1024, "未提及的搜索字段保留");
        Ok(())
    }

    /// 层1：覆盖层拼错键必须报错（不静默忽略）
    #[test]
    fn test_overlay_unknown_key_rejected() {
        let err = Config::parse(&base(), Some("[sim]\numaa = 1\n")).unwrap_err();
        assert!(format!("{err:#}").contains("umaa"), "未知名应出现在错误里");
    }

    /// 层1：约定覆盖层同名不同目录，存在才合并、缺席不报错
    #[test]
    fn test_overlay_file_convention() -> Result<()> {
        let tmp = umaai_core::testing::workspace_root()?.join("target/test-tmp/umaai-config-overlay");
        let root = tmp.join("data");
        let host = tmp.join("host");
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(&host)?;
        std::fs::write(root.join(CONFIG_FILE), base())?;

        let cfg = Config::load_with_overlay_file(&root, &host)?;
        assert_eq!(cfg.sim.uma, 100101, "覆盖层缺席时只用默认层，且不报错");

        std::fs::write(host.join(CONFIG_FILE), "[sim]\numa = 102601\n")?;
        let cfg = Config::load_with_overlay_file(&root, &host)?;
        assert_eq!(cfg.sim.uma, 102601, "覆盖层字段生效");
        assert_eq!(cfg.sim.seed, 42, "未提及字段沿用默认层");

        std::fs::remove_dir_all(&tmp)?;
        Ok(())
    }

    /// 层1：预设按名合并，同名覆盖、异名保留
    #[test]
    fn test_preset_merge() -> Result<()> {
        let overlay = format!("[presets.race_grades]\nextra = {}\n", grades());
        let cfg = Config::parse(&base(), Some(&overlay))?;
        assert_eq!(cfg.presets.race_grades.len(), 2, "同名合并而非整段替换");
        assert!(cfg.presets.race_grades.contains_key("standard"));
        assert!(cfg.presets.race_grades.contains_key("extra"));
        Ok(())
    }

    /// 层1：预设缺失与非法值必须报错（不静默兜底）
    #[test]
    fn test_preset_errors() {
        let bad_name = base().replace("race_grades = \"standard\"", "race_grades = \"nope\"");
        let msg = format!("{:#}", Config::parse(&bad_name, None).unwrap_err());
        assert!(
            msg.contains("nope") && msg.contains("standard"),
            "错误应列出可用预设: {msg}"
        );

        let short = base().replace(&grades(), "[0, 0, 0]");
        let msg = format!("{:#}", Config::parse(&short, None).unwrap_err());
        assert!(msg.contains("不足 72"), "长度不足应报错: {msg}");

        let bad_grade = base().replace(&grades(), &grades_with_first(5));
        let msg = format!("{:#}", Config::parse(&bad_grade, None).unwrap_err());
        assert!(msg.contains("非法等级"), "非法等级应报错: {msg}");
    }

    /// 层1：卡组重复与空卡组必须报错
    #[test]
    fn test_cards_validation() {
        let dup = base().replace("cards = [302424, 302464]", "cards = [302424, 302424]");
        let msg = format!("{:#}", Config::parse(&dup, None).unwrap_err());
        assert!(msg.contains("重复"), "重复卡应报错: {msg}");

        let empty = base().replace("cards = [302424, 302464]", "cards = []");
        let msg = format!("{:#}", Config::parse(&empty, None).unwrap_err());
        assert!(msg.contains("不能为空"), "空卡组应报错: {msg}");
    }
}
