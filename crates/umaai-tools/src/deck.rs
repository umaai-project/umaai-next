//! 组卡工具内核：卡池筛选 / 静态面板分 / 构成枚举 / 逐类型实测扫描
//!
//! 口径见 [`docs/plan/tools_deck_cardtest.md`](../../../docs/plan/tools_deck_cardtest.md)
//! （2026-10-18 拍板）：卡池只取 `cardId > 30250`、默认满破档、默认排除 30257（固有类型
//! 23 未实现）；剪枝默认走**逐类型实测扫描**——固定参考卡组其余槽位，只替换目标类型
//! 槽位，逐候选**同种子配对**跑批，用时序配对差分衡量单卡边际。
//!
//! 本模块只**列举与测量**：不改决策语义、不读配置层数值、不落盘。必带卡（如剧本友人）
//! 与构成约束由调用方（剧本层）通过 [`DeckSpec`] 注入。

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use umaai::Config;
use umaai_core::Data;
use umaai_core::gamedata::CardValue;
use umaai_core::stats::{self, Adjusted, Correct, Paired};
use umasim::game::base::basic::BasicGame;
use umasim::game::{Game, InheritInfo};
use umasim::trainer::RandomTrainer;

/// 卡池下限默认值：`cardId > 30250`（即首个入池 `30251`）
pub const DEFAULT_CARD_ID_MIN: u32 = 30251;
/// 排除名单默认值：30257 [友]塔可布莱恩（固有类型 23 未实现，归剧本侧）
pub const DEFAULT_EXCLUDE: [u32; 1] = [30257];
/// 满破档
pub const FULL_RANK: u32 = 4;
/// 默认训练卡槽位数（与默认参考卡组的规模一致：5 张训练卡）
///
/// 游戏规则是「卡组 6 张，通常含 1 张剧本友人」——必带卡由剧本层经 [`DeckSpec::fixed`]
/// 注入，故默认总槽位 = 本值 + 必带卡数。
pub const DEFAULT_TRAINING_SLOTS: usize = 5;
/// 每类型保留候选数默认值（构成枚举用）
pub const DEFAULT_TOP_K: usize = 5;
/// 实测扫描的每臂局数默认值
///
/// 依据（基础局 + 契约随机训练员实测）：同类型单卡替换的配对差标准差约 170–310，
/// 1000 对足以分辨约 25 分的效应；换成搜索型训练员时按同一分辨率折算局数。
pub const DEFAULT_SCAN_RUNS: u32 = 1000;

/// 默认参考卡组：旧仓 GA r5/r6 收敛的「万能骨架」（速2耐1智2，固有全为已实现类型）
///
/// 顺序即扫描槽位口径：某类型占多槽时，扫描取该类型**首个槽位**（见方案 §9 待决项）。
pub const DEFAULT_REFERENCE_DECK: [u32; 5] = [303124, 303174, 303044, 303064, 302894];

/// 观测轴
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// 终局评分
    Score,
    /// 终局 PT
    Pt,
    /// 五维之和（机读名沿用 CLI 的 `five`）
    #[serde(rename = "five")]
    FiveSum,
}

impl Metric {
    /// 解析观测轴名（`score` / `pt` / `five`）
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "score" => Ok(Self::Score),
            "pt" => Ok(Self::Pt),
            "five" => Ok(Self::FiveSum),
            other => anyhow::bail!("--metric 只认 score / pt / five（收到 \"{other}\"）"),
        }
    }

    /// 从一局终局取观测值
    #[must_use]
    pub fn of(&self, outcome: &Outcome) -> f64 {
        match self {
            Self::Score => f64::from(outcome.score),
            Self::Pt => f64::from(outcome.pt),
            Self::FiveSum => outcome.five.iter().map(|v| f64::from(*v)).sum(),
        }
    }
}

/// 一局终局观测
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// 终局评分
    pub score: i32,
    /// 终局 PT
    pub pt: i32,
    /// 终局五维
    pub five: [i32; 5],
}

/// 引擎侧卡组长度下限
///
/// 真实触发点是引擎支援卡事件生成里硬编码的人头上界 6（不是人头分配）：卡组短于 5 张时
/// 那个上界会取到不存在的人头；工具因此在入口**点名拒绝**过短卡组，不把它留给引擎
/// （见 `docs/issues.md` 的人头布局假设条目）。
pub const MIN_DECK_LEN: usize = 5;

/// 必带卡：固定槽位 + idrank
///
/// 剧本层用它注入「组卡时必须带的那张」（如剧本友人卡），扫描时按槽位拼进参考卡组。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedCard {
    /// 槽位下标（0 基，必须 < `DeckSpec::slots`）
    pub slot: usize,
    /// 卡 ID × 10 + 突破档
    pub idrank: u32,
}

/// 卡池与构成口径（剧本层注入；默认值见各常量）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckSpec {
    /// 卡池下限（含）
    pub card_id_min: u32,
    /// 排除名单（按 cardId 整卡排除）
    pub exclude: Vec<u32>,
    /// 突破档（0–4）
    pub rank: u32,
    /// 卡组槽位数
    pub slots: usize,
    /// 必带卡（槽位 + idrank；不参与扫描，只随参考卡组进组）
    pub fixed: Vec<FixedCard>,
    /// 单类型张数上限（`None` = 不限；剧本可 5 速）
    pub max_per_type: Option<usize>,
    /// 种类数下限（`None` = 不限；如拉面杯「种类 ≥4」）
    pub min_types: Option<usize>,
}

impl Default for DeckSpec {
    fn default() -> Self {
        Self {
            card_id_min: DEFAULT_CARD_ID_MIN,
            exclude: DEFAULT_EXCLUDE.to_vec(),
            rank: FULL_RANK,
            slots: DEFAULT_TRAINING_SLOTS,
            fixed: Vec::new(),
            max_per_type: None,
            min_types: None,
        }
    }
}

impl DeckSpec {
    /// 训练卡槽位数 = 总槽位 − 必带卡数
    #[must_use]
    pub fn training_slots(&self) -> usize {
        self.slots.saturating_sub(self.fixed.len())
    }

    /// 口径自检：突破档、槽位、必带卡坐标与重复
    pub fn validate(&self) -> Result<()> {
        ensure!(self.rank <= FULL_RANK, "突破档必须在 0..=4（收到 {}）", self.rank);
        ensure!(self.slots >= 1, "槽位数必须 ≥ 1（收到 {}）", self.slots);
        ensure!(
            self.fixed.len() <= self.slots,
            "必带卡 {} 张超过槽位数 {}",
            self.fixed.len(),
            self.slots
        );
        for fixed in &self.fixed {
            ensure!(
                fixed.slot < self.slots,
                "必带卡槽位 {} 越界（槽位 0..{}）",
                fixed.slot,
                self.slots
            );
        }
        let mut slots: Vec<usize> = self.fixed.iter().map(|f| f.slot).collect();
        slots.sort_unstable();
        let before = slots.len();
        slots.dedup();
        ensure!(slots.len() == before, "必带卡槽位重复");
        if let Some(cap) = self.max_per_type {
            ensure!(cap >= 1, "单类型张数上限必须 ≥ 1（收到 {cap}）");
        }
        if let Some(kinds) = self.min_types {
            ensure!(kinds >= 1, "种类数下限必须 ≥ 1（收到 {kinds}）");
        }
        Ok(())
    }
}

/// 静态面板分项与加权和
///
/// 加权和沿用旧仓口径 `友情×2 + 干劲×0.5 + 训练`，**只是剪枝与展示用的粗代理**，
/// 不含技能、不表示强度排名（旧仓代码注释同此声明）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Panel {
    /// 友情加成
    pub youqing: f32,
    /// 干劲加成
    pub ganjing: i32,
    /// 训练加成
    pub xunlian: i32,
    /// 得意率
    pub deyilv: f32,
    /// 初始羁绊
    pub jiban: i32,
    /// 赛后加成
    pub saihou: i32,
    /// 启发等级
    pub hint_level: i32,
    /// 面板加权和（友情×2 + 干劲×0.5 + 训练）
    pub score: f64,
}

impl Panel {
    /// 从某一档卡面取面板分项与加权和
    #[must_use]
    pub fn of(value: &CardValue) -> Self {
        Self {
            youqing: value.youqing,
            ganjing: value.ganjing,
            xunlian: value.xunlian,
            deyilv: value.deyilv,
            jiban: value.initial_jiban,
            saihou: value.saihou,
            hint_level: value.hint_level,
            score: f64::from(value.youqing) * 2.0 + f64::from(value.ganjing) * 0.5 + f64::from(value.xunlian),
        }
    }
}

/// 卡池条目（按指定突破档物化）
#[derive(Debug, Clone, PartialEq)]
pub struct CardRep {
    /// 卡 ID
    pub card_id: u32,
    /// 卡 ID × 10 + 突破档
    pub idrank: u32,
    /// 卡类型（0 速 / 1 耐 / 2 力 / 3 根 / 4 智 / 5 友人 / 6 团队）
    pub card_type: i32,
    /// 稀有度（1/2/3）
    pub rarity: u32,
    /// 卡名
    pub name: String,
    /// 该档面板
    pub panel: Panel,
}

/// 卡类型中文名（构成标签与呈现用）；越界类型给 `?`
#[must_use]
pub fn type_name_zh(card_type: i32) -> &'static str {
    match card_type {
        0 => "速",
        1 => "耐",
        2 => "力",
        3 => "根",
        4 => "智",
        5 => "友",
        6 => "团",
        _ => "?",
    }
}

/// 按口径取卡池：`cardId ≥ 下限`、排除名单、有指定突破档
///
/// 输出按 `(卡类型升序, 面板分降序, 卡 ID 升序)` 稳定排序，保证同输入同输出。
pub fn pool(spec: &DeckSpec) -> Result<Vec<CardRep>> {
    spec.validate()?;
    let data = Data::try_get().context("全局静态数据未初始化：入口应先载入数据根")?;
    let rank = spec.rank as usize;
    let mut out = Vec::new();
    for card in data.card.values() {
        if card.card_id < spec.card_id_min || spec.exclude.contains(&card.card_id) {
            continue;
        }
        let Some(value) = card.card_value.get(rank) else {
            continue;
        };
        out.push(CardRep {
            card_id: card.card_id,
            idrank: card.card_id * 10 + spec.rank,
            card_type: card.card_type,
            rarity: card.rarity,
            name: card.card_name.clone(),
            panel: Panel::of(value),
        });
    }
    ensure!(
        !out.is_empty(),
        "卡池为空：下限 {} / 排除 {} 张 / 突破档 {} —— 检查数据根与口径",
        spec.card_id_min,
        spec.exclude.len(),
        spec.rank
    );
    out.sort_by(|a, b| {
        a.card_type
            .cmp(&b.card_type)
            .then(b.panel.score.total_cmp(&a.panel.score))
            .then(a.card_id.cmp(&b.card_id))
    });
    Ok(out)
}

/// 一种类型构成：五类训练卡各几张
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Composition {
    /// 速 / 耐 / 力 / 根 / 智 的张数
    pub counts: [usize; 5],
}

impl Composition {
    /// 训练卡总数
    #[must_use]
    pub fn sum(&self) -> usize {
        self.counts.iter().sum()
    }

    /// 非零类型数
    #[must_use]
    pub fn kinds(&self) -> usize {
        self.counts.iter().filter(|c| **c > 0).count()
    }

    /// 中文标签，形如 `速2耐1智2`
    #[must_use]
    pub fn label(&self) -> String {
        let mut text = String::new();
        for (idx, count) in self.counts.iter().enumerate() {
            if *count > 0 {
                text.push_str(type_name_zh(idx as i32));
                text.push_str(&count.to_string());
            }
        }
        text
    }
}

/// 枚举全部合法构成（五类训练卡张数合计 = 训练卡槽位数）
///
/// 约束来自 [`DeckSpec`]：`max_per_type`（默认不限，故剧本可 5 速）与 `min_types`（默认不限）。
pub fn compositions(spec: &DeckSpec) -> Vec<Composition> {
    let total = spec.training_slots();
    let cap = spec.max_per_type.unwrap_or(total);
    let min_kinds = spec.min_types.unwrap_or(1);
    let mut out = Vec::new();
    let mut counts = [0usize; 5];
    enumerate(0, total, cap, min_kinds, &mut counts, &mut out);
    out
}

/// 递归枚举构成（`idx` = 当前属性下标，`left` = 剩余张数）
fn enumerate(
    idx: usize,
    left: usize,
    cap: usize,
    min_kinds: usize,
    counts: &mut [usize; 5],
    out: &mut Vec<Composition>,
) {
    if idx == 5 {
        if left == 0 {
            let comp = Composition { counts: *counts };
            if comp.kinds() >= min_kinds {
                out.push(comp);
            }
        }
        return;
    }
    for take in 0..=left.min(cap) {
        counts[idx] = take;
        enumerate(idx + 1, left - take, cap, min_kinds, counts, out);
    }
    counts[idx] = 0;
}

/// 组卡工具的流水线阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// 逐类型实测扫描（默认；产出各类型实测榜）
    Scan,
    /// 构成枚举 + 静态代表卡（不出实测）
    Compose,
    /// 排序：每构成一副代表卡组，粗筛 → 精评 → 独立种子集验证
    Rank,
    /// 扫描 + 排序（排序复用本次扫描结果，不重复扫）
    All,
}

impl Stage {
    /// 解析阶段名（`scan` / `compose` / `rank` / `all`）
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "scan" => Ok(Self::Scan),
            "compose" => Ok(Self::Compose),
            "rank" => Ok(Self::Rank),
            "all" => Ok(Self::All),
            other => anyhow::bail!("--stage 只认 scan / compose / rank / all（收到 \"{other}\"）"),
        }
    }

    /// 机读名
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Compose => "compose",
            Self::Rank => "rank",
            Self::All => "all",
        }
    }
}

/// 组卡实验计划：一份 JSON 自持一次实验（阶段 `--plan`，见方案 §4.4）
///
/// 与 CLI 的分工：计划里出现的实验变量**只认计划**——同名 CLI 开关同时给即由入口点名拒绝，
/// 不做「CLI 覆盖计划」的双来源合并；公共项（数据根 / 呈现宽度 / `--quiet`）不属实验变量。
/// `stage` 与 `seed` 必给：计划自身要可复现；其余字段缺省取本模块的常量，**不读配置层数值**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// 阶段（必给）
    pub stage: Stage,
    /// 起始种子（必给；第 i 局用 `seed + i`）
    pub seed: u64,
    /// 卡池下限（缺省 [`DEFAULT_CARD_ID_MIN`]）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_id_min: Option<u32>,
    /// 排除名单（缺省 [`DEFAULT_EXCLUDE`]；给空数组即清空）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<u32>>,
    /// 参考卡组（idrank；缺省 [`DEFAULT_REFERENCE_DECK`]）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed_deck: Option<Vec<u32>>,
    /// 卡组总槽位数（缺省 5 张训练卡 + 必带卡数）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<usize>,
    /// 必带卡（槽位 + idrank；不参与扫描，只随参考卡组进组）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixed: Vec<FixedCard>,
    /// 单类型张数上限（缺省不限）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_per_type: Option<usize>,
    /// 种类数下限（缺省不限）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_types: Option<usize>,
    /// 每臂局数（缺省 [`DEFAULT_SCAN_RUNS`]）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<u32>,
    /// 观测轴（缺省 `score`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<Metric>,
    /// 反推局数用的目标分辨率（缺省 [`crate::arms::DEFAULT_GOAL`]）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<f64>,
    /// 多重比较校正口径（缺省不校正）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correct: Option<Correct>,
    /// 每类型保留候选数（缺省 [`DEFAULT_TOP_K`]）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<usize>,
    /// 剪枝口径（缺省 `measured`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prune: Option<PruneMode>,
    /// 粗筛每臂局数（缺省 [`RankSpec::default`] 的取值）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_runs: Option<u32>,
    /// 进精评的候选数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fine_count: Option<usize>,
    /// 验证种子（缺省 `seed + 1000000`，必须与 `seed` 不同）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holdout_seed: Option<u64>,
    /// 进验证的候选数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holdout_count: Option<usize>,
    /// 备注（一行；随计划自持，不参与判据）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Plan {
    /// 计划自检：数值口径与「验证种子必须独立于选择集」
    pub fn validate(&self) -> Result<()> {
        if let Some(runs) = self.runs {
            ensure!(runs >= 1, "每臂局数必须 ≥ 1（收到 {runs}）");
        }
        if let Some(cap) = self.max_per_type {
            ensure!(cap >= 1, "单类型张数上限必须 ≥ 1（收到 {cap}）");
        }
        if let Some(kinds) = self.min_types {
            ensure!(kinds >= 1, "种类数下限必须 ≥ 1（收到 {kinds}）");
        }
        if let Some(k) = self.top_k {
            ensure!(k >= 1, "每类型候选数必须 ≥ 1（收到 {k}）");
        }
        if let Some(seed) = self.holdout_seed {
            ensure!(
                seed != self.seed,
                "验证种子必须独立于选择集（当前两者都是 {seed}）：否则验证等于自证"
            );
        }
        if self.correct.is_some() {
            ensure!(
                matches!(self.stage, Stage::Scan | Stage::All),
                "--correct 只对阶段 scan / all 有意义（计划里的阶段是 {}）",
                self.stage.name()
            );
        }
        Ok(())
    }
}

/// 读取并校验一份组卡实验计划
pub fn load_plan(path: &Path) -> Result<Plan> {
    let text = std::fs::read_to_string(path).with_context(|| format!("读取组卡实验计划失败: {}", path.display()))?;
    let plan: Plan =
        serde_json::from_str(&text).with_context(|| format!("解析组卡实验计划失败: {}", path.display()))?;
    plan.validate()?;
    Ok(plan)
}

/// 扫描规格
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanSpec {
    /// 每臂局数
    pub runs: u32,
    /// 起始种子（第 i 局用 `seed + i`，与 `batch` 同口径）
    pub seed: u64,
    /// 观测轴
    pub metric: Metric,
    /// 多重比较校正口径（族 = 该类型榜的全部候选）
    pub correct: Correct,
}

/// 单卡扫描结果（相对参考卡组）
#[derive(Debug, Clone, PartialEq)]
pub struct CardResult {
    /// 被替换进来的卡
    pub card: CardRep,
    /// 该臂观测均值
    pub mean: f64,
    /// 该臂观测标准差
    pub sd: f64,
    /// 与参考卡组的配对差分
    pub paired: Paired,
    /// 该类型榜内（含参考卡那一行）的多重比较校正结果
    pub adjusted: Adjusted,
    /// 是否就是参考卡组里那张（其配对差分恒为 0，用作管线自证）
    pub in_reference: bool,
}

/// 单类型榜
#[derive(Debug, Clone, PartialEq)]
pub struct TypeBoard {
    /// 卡类型
    pub card_type: i32,
    /// 被扫描的槽位下标（该类型在参考卡组里的首个槽位）
    pub slot: usize,
    /// 全部候选中位名次（1 基）；参考卡组那张卡的名次
    pub reference_rank: usize,
    /// 按配对差分降序的候选
    pub cards: Vec<CardResult>,
}

/// 扫描报告
#[derive(Debug, Clone)]
pub struct ScanReport {
    /// 完整参考卡组（idrank；训练卡 + 必带卡）
    pub reference: Vec<u32>,
    /// 卡池规模
    pub pool: usize,
    /// 每臂局数
    pub runs: u32,
    /// 起始种子
    pub seed: u64,
    /// 各类型榜
    pub boards: Vec<TypeBoard>,
    /// 参考卡组里没有、因而未出榜的类型（0 速 / 1 耐 / 2 力 / 3 根 / 4 智）
    pub skipped_types: Vec<i32>,
    /// 因与参考卡组其他槽位同卡（重复卡组）而跳过的候选数
    pub skipped_duplicates: usize,
    /// 整批墙钟耗时
    pub elapsed: Duration,
}

/// 把「训练卡序列 + 必带卡」按槽位布局成完整卡组（纯函数：不碰数据）
///
/// `training` 只给训练卡（默认 GA 骨架 5 张），必带卡由 [`DeckSpec::fixed`] 插到各自槽位。
/// 校验长度对齐（`training.len() + fixed.len() == slots`）、总长不低于引擎下限
/// [`MIN_DECK_LEN`]、卡组无重复。
pub fn layout_reference(spec: &DeckSpec, training: &[u32]) -> Result<Vec<u32>> {
    spec.validate()?;
    ensure!(!training.is_empty(), "参考卡组为空");
    ensure!(
        training.len() + spec.fixed.len() == spec.slots,
        "参考卡组长度不匹配：训练卡 {} 张 + 必带卡 {} 张 ≠ 槽位 {}（要改槽位用 --slots）",
        training.len(),
        spec.fixed.len(),
        spec.slots
    );
    ensure!(
        spec.slots >= MIN_DECK_LEN,
        "卡组至少 {MIN_DECK_LEN} 张（当前槽位 {}）：更短的卡组会让引擎的支援卡事件取到不存在的人头",
        spec.slots
    );
    let mut deck = vec![0_u32; spec.slots];
    let mut it = training.iter();
    for (slot, entry) in deck.iter_mut().enumerate() {
        if spec.fixed.iter().any(|f| f.slot == slot) {
            continue;
        }
        *entry = *it.next().context("训练卡数量与空槽位不匹配")?;
    }
    ensure!(it.next().is_none(), "训练卡数量与空槽位不匹配");
    for fixed in &spec.fixed {
        deck[fixed.slot] = fixed.idrank;
    }
    ensure_rank_legal(&deck)?;
    ensure!(
        unique_cards(&deck),
        "参考卡组存在重复卡（同一张卡占两个槽位，含同卡不同突破档）"
    );
    Ok(deck)
}

/// 逐槽突破档合法性（纯函数：不碰数据）
///
/// 与 [`deck_types`] 同口径、同文案；排在重复判定之前：非法档位是坏数据，
/// 先报它，免得被「重复卡」的结论盖过去。
fn ensure_rank_legal(deck: &[u32]) -> Result<()> {
    for (slot, idrank) in deck.iter().enumerate() {
        ensure!(
            idrank % 10 <= FULL_RANK,
            "第 {slot} 槽 idrank {idrank} 的突破档非法（需 0..=4）"
        );
    }
    Ok(())
}

/// 卡组内 `card_id` 是否两两不同（同卡不同突破档也算重复）
///
/// 与引擎构造期的判重同口径：人头 → 卡组的反查按 `card_id` 定位，重复会让两张人头都命中第一张。
#[must_use]
pub fn unique_cards(deck: &[u32]) -> bool {
    let mut ids: Vec<u32> = deck.iter().map(|idrank| idrank / 10).collect();
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    ids.len() == before
}

/// 布局 + 解析校验：每张卡的卡 ID 必须存在、突破档必须合法
pub fn build_reference(spec: &DeckSpec, training: &[u32]) -> Result<Vec<u32>> {
    let deck = layout_reference(spec, training)?;
    deck_types(&deck)?;
    Ok(deck)
}

/// 校验一副显式卡组（长度、重复、逐卡可解析），返回逐槽卡类型
///
/// 供显式给卡组的入口复用（测卡工具的基准卡组、剧本仓自持卡组）：长度低于引擎下限
/// [`MIN_DECK_LEN`] 会让引擎的支援卡事件取到不存在的人头（见 `docs/issues.md`），
/// 重复卡（含同卡不同突破档）会让反查静默命中第一张，两者都点名拒绝。
pub fn deck_types(deck: &[u32]) -> Result<Vec<i32>> {
    ensure!(
        deck.len() >= MIN_DECK_LEN,
        "卡组至少 {MIN_DECK_LEN} 张（当前 {} 张）：更短的卡组会让引擎的支援卡事件取到不存在的人头",
        deck.len()
    );
    ensure_rank_legal(deck)?;
    let mut types = Vec::with_capacity(deck.len());
    for (slot, idrank) in deck.iter().enumerate() {
        let card = Data::get()?
            .get_card(idrank / 10)
            .with_context(|| format!("第 {slot} 槽 idrank {idrank} 无法解析"))?;
        types.push(card.card_type);
    }
    ensure!(
        unique_cards(deck),
        "卡组存在重复卡（同一张卡占两个槽位，含同卡不同突破档）"
    );
    Ok(types)
}

/// 逐类型实测扫描：固定参考卡组其余槽位，只替换目标类型槽位
///
/// `training` 只给训练卡，必带卡由 [`DeckSpec::fixed`] 拼进参考卡组（见 [`build_reference`]）。
/// 每臂用**同一段种子序列**（第 i 局 `seed + i`），配对差分因此只含「换了一张卡」的贡献；
/// 参考卡组只跑一遍，全类型共用。异常局**不补采**（直接报错），保证配对键一致。
///
/// 三条入口守卫（都不静默）：参考卡组的每张训练卡必须在池内（否则零差分自证会失效）、
/// 与参考卡组其他槽位同卡的候选被跳过（那是重复卡组，游戏里组不出来）、全线零差分即报错。
pub fn scan_decks(cfg: &Config, spec: &DeckSpec, training: &[u32], scan_spec: &ScanSpec) -> Result<ScanReport> {
    ensure!(scan_spec.runs >= 1, "局数必须 ≥ 1");
    let reference = build_reference(spec, training)?;
    let reps = pool(spec)?;
    let grades = cfg.race_grades()?;
    let inherit = cfg.inherit();
    let started = Instant::now();

    // 参考卡组自身：逐位解析卡类型，并按首次出现顺序定扫描槽位
    let mut ref_types = Vec::with_capacity(reference.len());
    for id in &reference {
        let card = Data::get()?.get_card(id / 10)?;
        ref_types.push(card.card_type);
    }
    let mut types: Vec<i32> = Vec::new();
    for card_type in &ref_types {
        if (0..5).contains(card_type) && !types.contains(card_type) {
            types.push(*card_type);
        }
    }
    ensure!(!types.is_empty(), "参考卡组里没有训练系卡（类型 0–4），无法扫描");

    // 参考卡组里参与扫描的卡必须在池内：否则该类型的候选集里没有它，零差分自证静默失效
    for (slot, card_type) in ref_types.iter().enumerate() {
        if !(0..5).contains(card_type) {
            continue;
        }
        let card_id = reference[slot] / 10;
        ensure!(
            reps.iter().any(|r| r.card_id == card_id),
            "参考卡组第 {slot} 槽的卡 {card_id} 不在卡池内（被 --card-id-min / --exclude 排除？）：\
             该类型无法做零差分自证，请把它加回池或换参考卡"
        );
    }

    // 基准臂：全类型共用
    let base: Vec<f64> = (0..scan_spec.runs)
        .map(|i| run_deck(cfg, grades, &inherit, &reference, scan_spec.seed + u64::from(i)))
        .collect::<Result<Vec<Outcome>>>()?
        .iter()
        .map(|o| scan_spec.metric.of(o))
        .collect();

    let mut boards = Vec::with_capacity(types.len());
    let mut skipped_duplicates = 0_usize;
    for card_type in &types {
        let slot = ref_types
            .iter()
            .position(|t| t == card_type)
            .context("扫描槽位定位失败")?;
        let mut cards = Vec::new();
        for rep in reps.iter().filter(|r| r.card_type == *card_type) {
            // 与该类型其他槽位同卡的候选会造出重复卡组（游戏里组不出来），跳过并计数
            if reference
                .iter()
                .enumerate()
                .any(|(other, id)| other != slot && id / 10 == rep.card_id)
            {
                skipped_duplicates += 1;
                continue;
            }
            let mut deck = reference.clone();
            deck[slot] = rep.idrank;
            let arm: Vec<f64> = (0..scan_spec.runs)
                .map(|i| run_deck(cfg, grades, &inherit, &deck, scan_spec.seed + u64::from(i)))
                .collect::<Result<Vec<Outcome>>>()?
                .iter()
                .map(|o| scan_spec.metric.of(o))
                .collect();
            let paired = stats::paired(&base, &arm)?;
            let in_reference = deck == reference;
            if in_reference {
                ensure!(
                    paired.mean == 0.0 && paired.sd == 0.0,
                    "自证失败：参考卡组自身应给出零差分（实测 Δmean={} Δsd={}）",
                    paired.mean,
                    paired.sd
                );
            }
            cards.push(CardResult {
                card: rep.clone(),
                mean: stats::mean(&arm),
                sd: stats::stdev(&arm),
                paired,
                adjusted: Adjusted::neutral(),
                in_reference,
            });
        }
        // 校正以「该类型榜」为一族：同一次运行里互相比过的候选（含参考卡那一行）
        let family: Vec<Paired> = cards.iter().map(|card| card.paired).collect();
        for (card, adjusted) in cards.iter_mut().zip(scan_spec.correct.adjust(&family)) {
            card.adjusted = adjusted;
        }
        cards.sort_by(|a, b| {
            b.paired
                .mean
                .total_cmp(&a.paired.mean)
                .then(a.card.card_id.cmp(&b.card.card_id))
        });
        let reference_rank = cards.iter().position(|c| c.in_reference).map_or(0, |idx| idx + 1);
        ensure!(reference_rank >= 1, "类型 {card_type} 的榜里缺少参考卡（池内应含它）");
        boards.push(TypeBoard {
            card_type: *card_type,
            slot,
            reference_rank,
            cards,
        });
    }

    // 零差分自证只证明引擎可复现；若连一个候选都没测出差异，更可能是卡组根本没换进去
    let comparable: usize = boards.iter().map(|b| b.cards.len()).sum();
    let nonzero = boards
        .iter()
        .flat_map(|b| &b.cards)
        .filter(|c| !c.in_reference && c.paired.mean != 0.0)
        .count();
    ensure!(
        nonzero > 0 || comparable <= boards.len(),
        "扫描结果全线零差分：候选卡组可能没有真正注入（检查卡池与槽位映射）"
    );

    let skipped_types: Vec<i32> = (0..5).filter(|t| !types.contains(t)).collect();
    Ok(ScanReport {
        reference,
        pool: reps.len(),
        runs: scan_spec.runs,
        seed: scan_spec.seed,
        boards,
        skipped_types,
        skipped_duplicates,
        elapsed: started.elapsed(),
    })
}

/// 跑指定卡组的一局（规则主种子与决策流同源，与 `batch` 的「第 i 局 seed + i」同口径）
fn run_deck(cfg: &Config, grades: &[i32], inherit: &InheritInfo, deck: &[u32], seed: u64) -> Result<Outcome> {
    let mut game = BasicGame::newgame(cfg.sim.uma, deck, inherit.clone(), grades)?;
    game.set_rule_master(seed);
    let mut rng = StdRng::seed_from_u64(seed);
    game.run_full_game(&RandomTrainer, &mut rng)?;
    let uma = game.uma();
    Ok(Outcome {
        score: uma.calc_score()?,
        pt: uma.total_pt()?,
        five: uma.five_status,
    })
}

/// 跑一局并取观测轴上的值（基础局档的**测量函数**；剧本仓用自己的装配 + 手写训练员替换它）
///
/// 与 [`crate::arms::run`] 的签名配套：`measure(deck, seed) -> 观测值`。
pub fn measure(cfg: &Config, deck: &[u32], seed: u64, metric: Metric) -> Result<f64> {
    let grades = cfg.race_grades()?;
    let inherit = cfg.inherit();
    let outcome = run_deck(cfg, grades, &inherit, deck, seed)?;
    Ok(metric.of(&outcome))
}

/// 按类型分组卡池（保持池内顺序：同类型内已是面板分降序）
#[must_use]
pub fn order_by_type(reps: &[CardRep]) -> [Vec<u32>; 5] {
    let mut out: [Vec<u32>; 5] = std::array::from_fn(|_| Vec::new());
    for rep in reps {
        if (0..5).contains(&rep.card_type) {
            out[rep.card_type as usize].push(rep.idrank);
        }
    }
    out
}

/// 按实测扫描结果给出每类型的卡序（配对增量降序）
#[must_use]
pub fn measured_order(report: &ScanReport) -> [Vec<u32>; 5] {
    let mut out: [Vec<u32>; 5] = std::array::from_fn(|_| Vec::new());
    for board in &report.boards {
        out[board.card_type as usize] = board.cards.iter().map(|c| c.card.idrank).collect();
    }
    out
}

/// 实测排名 + **缺榜类型回退静态面板序**（返回卡序与兜底类型）
///
/// 参考卡组里没有的类型（如默认骨架缺力 / 根）不会出榜，若直接留空会让所有用到该类型的构成
/// 装配失败、被静默跳过——故回退到静态序并**把兜底类型报出去**，不静默降级。
fn pruned_order(reps: &[CardRep], report: &ScanReport) -> ([Vec<u32>; 5], Vec<i32>) {
    let mut ordered = measured_order(report);
    let static_order = order_by_type(reps);
    let mut fallback = Vec::new();
    for (idx, list) in ordered.iter_mut().enumerate() {
        if list.is_empty() && !static_order[idx].is_empty() {
            *list = static_order[idx].clone();
            fallback.push(idx as i32);
        }
    }
    (ordered, fallback)
}

/// 按构成装配一副卡组：每类型取该类型卡序的前 `counts[i]` 张，必带卡按槽位插入
pub fn assemble(spec: &DeckSpec, comp: &Composition, ordered: &[Vec<u32>; 5]) -> Result<Vec<u32>> {
    let mut training = Vec::with_capacity(comp.sum());
    for (idx, count) in comp.counts.iter().enumerate() {
        ensure!(
            ordered[idx].len() >= *count,
            "{} 类型的卡序不足：需要 {count} 张、只有 {} 张",
            type_name_zh(idx as i32),
            ordered[idx].len()
        );
        training.extend_from_slice(&ordered[idx][..*count]);
    }
    layout_reference(spec, &training)
}

/// 剪枝口径（代表卡从哪来）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PruneMode {
    /// 静态面板分（池内顺序；秒级、可解释）
    Static,
    /// 逐类型实测扫描排名（贵一档，但按实测强弱取代表卡）
    Measured,
}

impl PruneMode {
    /// 解析口径名（`static` / `measured`）
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "static" => Ok(Self::Static),
            "measured" => Ok(Self::Measured),
            other => anyhow::bail!("--prune 只认 static / measured（收到 \"{other}\"）"),
        }
    }

    /// 机读名
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Measured => "measured",
        }
    }
}

/// 排序阶段规格（两级 + 独立种子集验证）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RankSpec {
    /// 剪枝口径（代表卡从哪来）
    pub prune: PruneMode,
    /// `Measured` 剪枝时每类型保留的候选数（扫描排名前 K 在内）
    pub top_k: usize,
    /// `Measured` 剪枝时扫描的每臂局数
    pub scan_runs: u32,
    /// 粗筛每臂局数
    pub screen_runs: u32,
    /// 精评每臂局数
    pub runs: u32,
    /// 进入精评的候选数
    pub fine_count: usize,
    /// 验证每臂局数
    pub holdout_runs: u32,
    /// 进入验证的候选数
    pub holdout_count: usize,
    /// 粗筛 / 精评的起始种子
    pub seed: u64,
    /// 验证的起始种子（**必须独立于选择集**）
    pub holdout_seed: u64,
    /// 观测轴
    pub metric: Metric,
    /// 反推局数用的目标分辨率
    pub goal: f64,
}

impl Default for RankSpec {
    fn default() -> Self {
        Self {
            prune: PruneMode::Measured,
            top_k: DEFAULT_TOP_K,
            scan_runs: 200,
            screen_runs: 20,
            runs: 200,
            fine_count: 10,
            holdout_runs: 200,
            holdout_count: 3,
            seed: 0,
            holdout_seed: 1_000_000,
            metric: Metric::Score,
            goal: crate::arms::DEFAULT_GOAL,
        }
    }
}

impl RankSpec {
    /// 规格自检
    fn validate(&self) -> Result<()> {
        ensure!(self.top_k >= 1, "每类型候选数必须 ≥ 1（收到 {}）", self.top_k);
        ensure!(self.scan_runs >= 1, "扫描局数必须 ≥ 1（收到 {}）", self.scan_runs);
        ensure!(self.screen_runs >= 1, "粗筛局数必须 ≥ 1（收到 {}）", self.screen_runs);
        ensure!(self.runs >= 1, "精评局数必须 ≥ 1（收到 {}）", self.runs);
        ensure!(
            self.fine_count >= 1,
            "进精评的候选数必须 ≥ 1（收到 {}）",
            self.fine_count
        );
        ensure!(self.holdout_runs >= 1, "验证局数必须 ≥ 1（收到 {}）", self.holdout_runs);
        ensure!(self.goal > 0.0, "目标分辨率必须 > 0（收到 {}）", self.goal);
        ensure!(
            self.seed != self.holdout_seed,
            "验证种子必须独立于选择集（当前两者都是 {}）：否则验证等于自证",
            self.seed
        );
        Ok(())
    }
}

/// 单个候选的测量结果
#[derive(Debug, Clone, PartialEq)]
pub struct Measured {
    /// 观测均值
    pub mean: f64,
    /// 观测样本标准差
    pub sd: f64,
    /// 相对参考卡组的配对差分
    pub paired: Paired,
    /// 按本臂 Δsd 反推的所需配对数
    pub runs_for_goal: u32,
}

/// 一行候选卡组
#[derive(Debug, Clone, PartialEq)]
pub struct RankRow {
    /// 卡组（idrank）
    pub deck: Vec<u32>,
    /// 类型构成
    pub composition: Composition,
    /// 卡组静态面板分之和（诊断列，不参与排序）
    pub panel_score: f64,
    /// 粗筛结果
    pub screen: Measured,
    /// 精评结果（进前 `fine_count` 才有）
    pub fine: Option<Measured>,
    /// 独立种子集验证结果（进前 `holdout_count` 才有）
    pub holdout: Option<Measured>,
}

impl RankRow {
    /// 该行当前可用的最好测量值（**诊断用，不用于排序**）
    ///
    /// 粗筛与精评的局数不同，两个 Δ 不是一把尺子，直接比大小会把「精评后掉下来的行」错排到
    /// 「只粗筛的行」后面；排序一律走 [`rank_order`] 的两级口径。
    #[must_use]
    pub fn best_delta(&self) -> f64 {
        self.fine.as_ref().map_or(self.screen.paired.mean, |m| m.paired.mean)
    }

    /// 测量层级：`1` = 已精评，`0` = 仅粗筛
    #[must_use]
    pub fn tier(&self) -> u8 {
        u8::from(self.fine.is_some())
    }

    /// 层级中文名（呈现用）
    #[must_use]
    pub fn tier_name(&self) -> &'static str {
        if self.fine.is_some() { "精评" } else { "仅粗筛" }
    }
}

/// 两级排序：已精评的整段排在仅粗筛的之前，段内各自按自己的 Δ 降序
///
/// 不可跨层比大小——粗筛少局数、精评多局数，混排等于用两把尺子量同一列。
fn rank_order(a: &RankRow, b: &RankRow) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a.fine.as_ref(), b.fine.as_ref()) {
        (Some(x), Some(y)) => y.paired.mean.total_cmp(&x.paired.mean).then(a.deck.cmp(&b.deck)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => b
            .screen
            .paired
            .mean
            .total_cmp(&a.screen.paired.mean)
            .then(a.deck.cmp(&b.deck)),
    }
}

/// 排序报告
#[derive(Debug, Clone)]
pub struct RankReport {
    /// 参考卡组（idrank；配对基准）
    pub reference: Vec<u32>,
    /// 卡池规模
    pub pool: usize,
    /// 剪枝口径
    pub prune: PruneMode,
    /// 候选卡组数（一种构成一副代表卡组）
    pub candidates: usize,
    /// 因卡序不足而跳过的构成数
    pub skipped_unfillable: usize,
    /// `measured` 剪枝下**回退到静态序**的类型（参考卡组里没有该类型，故没有实测榜）
    pub fallback_types: Vec<i32>,
    /// 规格
    pub spec: RankSpec,
    /// 按 [`rank_order`] 的两级口径排序（精评段在前）
    pub rows: Vec<RankRow>,
    /// 整批墙钟耗时
    pub elapsed: Duration,
}

/// 排序阶段：每个合法构成装配一副**代表卡组**，粗筛 → 精评 → 独立种子集验证
///
/// 候选构造沿用旧仓 `bench_compositions` 的口径（一种构成一副卡组，代表卡由剪枝口径给出），
/// 因此候选数是**构成数**而不是全空间——新池全空间约 965 万副，不做穷举。
///
/// 三级测量都经 [`crate::arms::run`] 走同一套同种子配对；选择集（`seed`）与验收集
/// （`holdout_seed`）必须不同，且验证结果**只复验、不参与排序**。
/// `scan` 已算好时传进来复用，避免 `all` 阶段重复扫描；`None` 表示按需自算
pub fn rank(
    cfg: &Config,
    spec: &DeckSpec,
    training: &[u32],
    rank_spec: &RankSpec,
    scan: Option<&ScanReport>,
) -> Result<RankReport> {
    rank_spec.validate()?;
    let reference = build_reference(spec, training)?;
    let reps = pool(spec)?;
    let started = Instant::now();

    let (ordered, fallback_types) = match rank_spec.prune {
        PruneMode::Static => (order_by_type(&reps), Vec::new()),
        PruneMode::Measured => match scan {
            Some(report) => pruned_order(&reps, report),
            None => {
                let report = scan_decks(
                    cfg,
                    spec,
                    training,
                    &ScanSpec {
                        runs: rank_spec.scan_runs,
                        seed: rank_spec.seed,
                        metric: rank_spec.metric,
                        // 剪枝内部这次扫描只用来定代表卡顺序，报告不出它的 CI，故不校正
                        correct: Correct::None,
                    },
                )?;
                pruned_order(&reps, &report)
            }
        },
    };

    // 候选：一种构成一副代表卡组（卡序不足的构成跳过并计数）
    let mut candidates: Vec<(Composition, Vec<u32>)> = Vec::new();
    let mut skipped_unfillable = 0_usize;
    for comp in compositions(spec) {
        match assemble(spec, &comp, &ordered) {
            Ok(deck) => candidates.push((comp, deck)),
            Err(_) => skipped_unfillable += 1,
        }
    }
    ensure!(!candidates.is_empty(), "没有可装配的候选卡组（检查卡池与构成约束）");

    let measure = |deck: &[u32], seed: u64| measure(cfg, deck, seed, rank_spec.metric);
    // 粗筛：基准臂在首位（`arms::run` 约定首臂为基准）
    let mut decks = Vec::with_capacity(candidates.len() + 1);
    decks.push(reference.clone());
    decks.extend(candidates.iter().map(|(_, deck)| deck.clone()));
    let screen = crate::arms::run(
        crate::arms::Spec {
            runs: rank_spec.screen_runs,
            seed: rank_spec.seed,
        },
        &decks,
        rank_spec.goal,
        measure,
    )?;
    let mut rows: Vec<RankRow> = candidates
        .iter()
        .zip(screen.results.iter())
        .map(|((comp, deck), result)| RankRow {
            deck: deck.clone(),
            composition: *comp,
            panel_score: panel_sum(&reps, deck),
            screen: to_measured(result),
            fine: None,
            holdout: None,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.screen
            .paired
            .mean
            .total_cmp(&a.screen.paired.mean)
            .then(a.deck.cmp(&b.deck))
    });

    // 精评：取粗筛前列，重跑更长局数（同一种子集）
    let fine_count = rank_spec.fine_count.min(rows.len());
    let mut fine_decks = Vec::with_capacity(fine_count + 1);
    fine_decks.push(reference.clone());
    fine_decks.extend(rows[..fine_count].iter().map(|r| r.deck.clone()));
    let fine = crate::arms::run(
        crate::arms::Spec {
            runs: rank_spec.runs,
            seed: rank_spec.seed,
        },
        &fine_decks,
        rank_spec.goal,
        measure,
    )?;
    for (row, result) in rows[..fine_count].iter_mut().zip(fine.results.iter()) {
        row.fine = Some(to_measured(result));
    }
    rows.sort_by(rank_order);

    // 验证：独立种子集，只复验不参与选择
    let holdout_count = rank_spec.holdout_count.min(rows.len());
    let mut holdout_decks = Vec::with_capacity(holdout_count + 1);
    holdout_decks.push(reference.clone());
    holdout_decks.extend(rows[..holdout_count].iter().map(|r| r.deck.clone()));
    let holdout = crate::arms::run(
        crate::arms::Spec {
            runs: rank_spec.holdout_runs,
            seed: rank_spec.holdout_seed,
        },
        &holdout_decks,
        rank_spec.goal,
        measure,
    )?;
    for (row, result) in rows[..holdout_count].iter_mut().zip(holdout.results.iter()) {
        row.holdout = Some(to_measured(result));
    }

    Ok(RankReport {
        reference,
        pool: reps.len(),
        prune: rank_spec.prune,
        candidates: rows.len(),
        skipped_unfillable,
        fallback_types,
        spec: *rank_spec,
        rows,
        elapsed: started.elapsed(),
    })
}

/// 把内核的一臂结果转成本模块的测量结果
fn to_measured(result: &crate::arms::ArmResult) -> Measured {
    Measured {
        mean: result.arm.mean(),
        sd: result.arm.sd(),
        paired: result.paired,
        runs_for_goal: result.runs_for_goal,
    }
}

/// 卡组的静态面板分之和（诊断列）
fn panel_sum(reps: &[CardRep], deck: &[u32]) -> f64 {
    deck.iter()
        .filter_map(|id| reps.iter().find(|r| r.idrank == *id))
        .map(|r| r.panel.score)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：面板加权和按旧仓口径（友情×2 + 干劲×0.5 + 训练）
    #[test]
    fn test_panel_score() {
        let value = CardValue {
            youqing: 30.0,
            ganjing: 40,
            xunlian: 10,
            deyilv: 100.0,
            initial_jiban: 30,
            saihou: 10,
            hint_level: 3,
            hint_prob_increase: 0,
            wiz_vital_bonus: 0,
            fail_rate_drop: 0.0,
            vital_cost_drop: 0.0,
            event_effect_up: 0,
            event_recovery_amount_up: 0,
            bonus: [0; 6],
            initial_bonus: [0; 6],
            hint_bonus: [0; 6],
        };
        let panel = Panel::of(&value);
        assert!((panel.score - (30.0 * 2.0 + 40.0 * 0.5 + 10.0)).abs() < 1e-9);
        assert_eq!(panel.deyilv, 100.0);
        assert_eq!(panel.jiban, 30);
    }

    /// 层1：5 张训练卡无上限时构成数 = C(9,4) = 126；每类型 ≤3 时退化为旧仓的 101
    #[test]
    fn test_compositions_counts() {
        let spec = DeckSpec::default();
        assert_eq!(spec.training_slots(), 5, "默认即 5 张训练卡（+ 必带卡）");
        assert_eq!(compositions(&spec).len(), 126, "五类合计 5 张的全部构成");
        let capped = DeckSpec {
            max_per_type: Some(3),
            ..spec.clone()
        };
        assert_eq!(compositions(&capped).len(), 101, "旧仓 bench_compositions 口径");
        // 6 槽（5 训练 + 1 必带）同样是 5 张训练卡 → 构成数不变
        let with_friend = DeckSpec {
            slots: 6,
            fixed: vec![FixedCard { slot: 5, idrank: 303_054 }],
            ..spec.clone()
        };
        assert_eq!(with_friend.training_slots(), 5);
        assert_eq!(compositions(&with_friend).len(), 126);
    }

    /// 层1：构成合法性与标签；必带卡占槽后训练槽位随之减少
    #[test]
    fn test_composition_legality_and_label() {
        let spec = DeckSpec {
            slots: 6,
            fixed: vec![FixedCard { slot: 5, idrank: 303_054 }],
            max_per_type: Some(5),
            min_types: Some(4),
            ..DeckSpec::default()
        };
        assert_eq!(spec.training_slots(), 5);
        let comps = compositions(&spec);
        assert!(!comps.is_empty());
        for comp in &comps {
            assert_eq!(comp.sum(), 5, "构成合计应等于训练卡槽位");
            assert!(comp.kinds() >= 4, "种类下限未被遵守: {}", comp.label());
        }
        let label = Composition { counts: [2, 1, 0, 0, 2] }.label();
        assert_eq!(label, "速2耐1智2");
    }

    /// 层1：口径自检拦住越界突破档、重复槽位与越界槽位
    #[test]
    fn test_spec_validate() {
        let bad_rank = DeckSpec { rank: 5, ..DeckSpec::default() };
        assert!(bad_rank.validate().is_err(), "突破档 5 应报错");
        let dup = DeckSpec {
            fixed: vec![FixedCard { slot: 1, idrank: 1 }, FixedCard { slot: 1, idrank: 2 }],
            ..DeckSpec::default()
        };
        assert!(dup.validate().is_err(), "必带卡槽位重复应报错");
        let oob = DeckSpec {
            slots: 2,
            fixed: vec![FixedCard { slot: 2, idrank: 1 }],
            ..DeckSpec::default()
        };
        assert!(oob.validate().is_err(), "必带卡槽位越界应报错");
        assert!(DeckSpec::default().validate().is_ok());
    }

    /// 层1：观测轴解析与取值
    #[test]
    fn test_metric() {
        let outcome = Outcome {
            score: 16_000,
            pt: 3_000,
            five: [1000, 900, 800, 700, 600],
        };
        assert_eq!(Metric::parse("score").expect("score").of(&outcome), 16_000.0);
        assert_eq!(Metric::parse("pt").expect("pt").of(&outcome), 3_000.0);
        assert_eq!(Metric::parse("five").expect("five").of(&outcome), 4_000.0);
        assert!(Metric::parse("nope").is_err());
    }

    /// 层1：默认参考卡组为速2耐1智2，且无重复
    #[test]
    fn test_default_reference_deck() {
        assert_eq!(DEFAULT_REFERENCE_DECK.len(), 5);
        let mut uniq = DEFAULT_REFERENCE_DECK.to_vec();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), DEFAULT_REFERENCE_DECK.len(), "参考卡组不得重复");
    }

    /// 层1：布局把必带卡插到指定槽位、训练卡按序填空位
    #[test]
    fn test_layout_reference_splices_fixed() {
        let spec = DeckSpec {
            slots: 6,
            fixed: vec![FixedCard { slot: 3, idrank: 303_054 }],
            ..DeckSpec::default()
        };
        let training = [303_124_u32, 303_174, 303_044, 303_064, 302_894];
        let deck = layout_reference(&spec, &training).expect("布局应成立");
        assert_eq!(
            deck,
            vec![303_124, 303_174, 303_044, 303_054, 303_064, 302_894],
            "必带卡占第 3 槽，训练卡按序让位"
        );
    }

    /// 层1：按类型分组保持池内顺序；装配按构成取每类型前 N 张
    #[test]
    fn test_order_and_assemble() {
        let rep = |idrank: u32, card_type: i32, score: f64| CardRep {
            card_id: idrank / 10,
            idrank,
            card_type,
            rarity: 3,
            name: format!("#{idrank}"),
            panel: Panel { score, ..Panel::default() },
        };
        let reps = vec![
            rep(10, 0, 90.0),
            rep(20, 0, 80.0),
            rep(30, 1, 70.0),
            rep(40, 4, 60.0),
            rep(50, 4, 50.0),
        ];
        let ordered = order_by_type(&reps);
        assert_eq!(ordered[0], vec![10, 20], "同类型保持池内顺序");
        assert_eq!(ordered[1], vec![30]);
        assert_eq!(ordered[2], Vec::<u32>::new());
        assert_eq!(ordered[4], vec![40, 50]);

        let spec = DeckSpec::default();
        let comp = Composition { counts: [3, 1, 0, 0, 1] };
        let err = assemble(&spec, &comp, &ordered).expect_err("速卡不足应报错");
        assert!(format!("{err:#}").contains("卡序不足"), "{err:#}");

        let comp = Composition { counts: [2, 1, 0, 0, 2] };
        let deck = assemble(&spec, &comp, &ordered).expect("装配应成立");
        assert_eq!(deck, vec![10, 20, 30, 40, 50], "训练卡按类型顺序填入槽位");
    }

    /// 层1：布局拦住长度不匹配、过短卡组与重复卡
    #[test]
    fn test_layout_reference_guards() {
        // 训练卡 + 必带卡 ≠ 槽位
        let spec = DeckSpec {
            slots: 6,
            ..DeckSpec::default()
        };
        assert!(
            layout_reference(&spec, &[1, 2, 3, 4, 5]).is_err(),
            "5 张训练卡对 6 槽应报错"
        );
        // 低于引擎下限（少于 5 张会让支援卡事件取到不存在的人头，见 docs/issues.md）
        let tiny = DeckSpec {
            slots: 4,
            ..DeckSpec::default()
        };
        let err = layout_reference(&tiny, &[1, 2, 3, 4]).expect_err("过短卡组应报错");
        assert!(format!("{err:#}").contains("至少 5 张"), "错误要点名下限: {err:#}");
        // 重复卡：同卡同档与**同卡不同突破档**都算重复（与引擎构造期同口径）
        let spec = DeckSpec::default();
        assert!(layout_reference(&spec, &[1, 1, 2, 3, 4]).is_err(), "同卡占两槽应报错");
        let err =
            layout_reference(&spec, &[303_124, 303_123, 302_894, 303_044, 303_174]).expect_err("同卡不同档应报错");
        assert!(
            format!("{err:#}").contains("同卡不同突破档"),
            "错误要点明同卡不同档也算重复: {err:#}"
        );
        assert!(unique_cards(&[303_124, 303_174]), "不同卡应判为无重复");
        assert!(!unique_cards(&[303_124, 303_124]), "同卡同档应判为重复");
        assert!(!unique_cards(&[303_124, 303_123]), "同卡不同档应判为重复");
        assert!(unique_cards(&DEFAULT_REFERENCE_DECK), "默认参考卡组无重复");
        // 正常路径
        assert!(layout_reference(&spec, &DEFAULT_REFERENCE_DECK).is_ok());
    }

    /// 层1：阶段名解析与机读名一一对应
    #[test]
    fn test_stage_names() -> Result<()> {
        for (raw, want) in [
            ("scan", Stage::Scan),
            ("compose", Stage::Compose),
            ("rank", Stage::Rank),
            ("all", Stage::All),
        ] {
            let stage = Stage::parse(raw)?;
            assert_eq!(stage, want);
            assert_eq!(stage.name(), raw, "机读名与开关取值同名");
        }
        assert!(Stage::parse("nope").is_err());
        Ok(())
    }

    /// 层1：计划 JSON 严格解析——stage / seed 必给，拼错键与缺字段都要被看见
    #[test]
    fn test_plan_serde_strict() -> Result<()> {
        let plan: Plan = serde_json::from_str(r#"{"stage": "scan", "seed": 42}"#)?;
        assert_eq!(plan.stage, Stage::Scan);
        assert_eq!(plan.seed, 42);
        assert!(plan.card_id_min.is_none() && plan.fixed.is_empty(), "缺省项走工具常量");
        assert!(plan.validate().is_ok());

        assert!(
            serde_json::from_str::<Plan>(r#"{"stage": "scan", "seed": 1, "run": 10}"#).is_err(),
            "拼错的键必须被看见"
        );
        assert!(
            serde_json::from_str::<Plan>(r#"{"seed": 1}"#).is_err(),
            "缺 stage 应报错"
        );
        assert!(
            serde_json::from_str::<Plan>(r#"{"stage": "scan"}"#).is_err(),
            "缺 seed 应报错（计划自持可复现）"
        );
        assert!(
            serde_json::from_str::<Plan>(r#"{"stage": "sort", "seed": 1}"#).is_err(),
            "阶段取值只认四个阶段"
        );
        // 观测轴与校正口径的机读名（`five` 不是 `five_sum`）
        let plan: Plan = serde_json::from_str(r#"{"stage": "all", "seed": 1, "metric": "five", "correct": "holm"}"#)?;
        assert_eq!(plan.metric, Some(Metric::FiveSum));
        assert_eq!(plan.correct, Some(Correct::Holm));
        // 必带卡是带键的结构（不是裸二元组）
        let plan: Plan = serde_json::from_str(
            r#"{"stage": "scan", "seed": 1, "fixed": [{"slot": 5, "idrank": 303054}], "slots": 6}"#,
        )?;
        assert_eq!(plan.fixed, vec![FixedCard { slot: 5, idrank: 303_054 }]);
        assert!(plan.validate().is_ok());
        Ok(())
    }

    /// 层1：计划自检拦住零局数 / 零候选数与不独立的选择-验证种子
    #[test]
    fn test_plan_validate() {
        let base = Plan {
            stage: Stage::Scan,
            seed: 7,
            card_id_min: None,
            exclude: None,
            seed_deck: None,
            slots: None,
            fixed: Vec::new(),
            max_per_type: None,
            min_types: None,
            runs: None,
            metric: None,
            goal: None,
            correct: None,
            top_k: None,
            prune: None,
            screen_runs: None,
            fine_count: None,
            holdout_seed: None,
            holdout_count: None,
            note: None,
        };
        assert!(base.validate().is_ok());
        assert!(Plan { runs: Some(0), ..base.clone() }.validate().is_err());
        assert!(Plan { top_k: Some(0), ..base.clone() }.validate().is_err());
        assert!(
            Plan {
                max_per_type: Some(0),
                ..base.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            Plan {
                holdout_seed: Some(7),
                ..base.clone()
            }
            .validate()
            .is_err(),
            "验证种子不得与选择集相同"
        );
        assert!(Plan { holdout_seed: Some(8), ..base }.validate().is_ok());
    }
}
