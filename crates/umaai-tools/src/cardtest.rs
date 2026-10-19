//! 测卡工具内核：固定基准卡组、只换一个槽位，逐候选同种子配对测单卡边际
//!
//! 与 [`crate::deck::scan_decks`] 的分工：扫描按**卡池 + 参考卡组**自动出各类型榜，本模块收**显式
//! 候选 idrank 列表**（可含不同突破档），是通用量具；两者共用 [`crate::arms`] 与 [`crate::deck::measure`]
//! 的配对口径，因此剧本仓换成自己的手写训练员时结论仍可比。
//!
//! 三条守卫（都不静默）：基准卡组长度低于引擎下限、候选与基准卡**类型不同**（那是构成效应
//! 而非单卡效应）、候选会造出**重复卡组**（与基准卡组其他槽位同卡）。

use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use umaai::Config;
use umaai_core::Data;
use umaai_core::stats::{Adjusted, Correct, Paired};

use crate::arms;
use crate::deck::{self, Measured, Metric};

/// 测卡规格（单槽控变量）
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    /// 变动槽位（0 基）
    pub slot: usize,
    /// 候选 idrank（逗号分隔的显式列表；可含不同突破档）
    pub candidates: Vec<u32>,
    /// 每臂局数
    pub runs: u32,
    /// 起始种子（第 i 局用 `seed + i`）
    pub seed: u64,
    /// 观测轴
    pub metric: Metric,
    /// 反推局数用的目标分辨率
    pub goal: f64,
    /// 允许候选与基准卡类型不同（默认拒绝：类型变了测的就是构成效应）
    pub allow_cross_type: bool,
    /// 多重比较校正口径（族 = 本次运行的全部候选）
    pub correct: Correct,
}

/// 单候选结果
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// 候选 idrank
    pub idrank: u32,
    /// 卡 ID
    pub card_id: u32,
    /// 卡名
    pub name: String,
    /// 卡类型
    pub card_type: i32,
    /// 该臂是否就是基准卡组（配对增量恒为 0，用作管线自证）
    pub is_reference: bool,
    /// 测量结果
    pub measured: Measured,
    /// 本次运行候选族内的多重比较校正结果
    pub adjusted: Adjusted,
}

/// 测卡报告
#[derive(Debug, Clone)]
pub struct Report {
    /// 基准卡组（idrank）
    pub base: Vec<u32>,
    /// 变动槽位
    pub slot: usize,
    /// 基准卡在该槽位的卡类型
    pub base_type: i32,
    /// 规格
    pub spec: Spec,
    /// 按配对增量降序的候选
    pub rows: Vec<Row>,
    /// 整批墙钟耗时
    pub elapsed: Duration,
}

impl Report {
    /// 基准卡本身那一行（用于自证与展示）
    #[must_use]
    pub fn reference(&self) -> Option<&Row> {
        self.rows.iter().find(|row| row.is_reference)
    }
}

/// 候选可比性判定（纯函数：不碰数据，便于单测）
///
/// 三条规则：① 类型必须与基准槽位一致（否则测的是构成效应），除非显式放行；
/// ② 候选的同 `card_id` 卡不能已占基准卡组的**其他**槽位（换进来即重复卡组，同卡不同突破档也不行）；
/// ③ 候选就是基准卡组该槽位的卡时放行（它就是自证用的零差分臂）。
fn check_candidate(
    base: &[u32],
    slot: usize,
    base_type: i32,
    idrank: u32,
    card_type: i32,
    name: &str,
    allow_cross_type: bool,
) -> Result<()> {
    if !allow_cross_type {
        ensure!(
            card_type == base_type,
            "{name} 是{}类型，与基准槽位上的{}类型不同——类型变了测的是构成效应而非单卡效应；\
             确实要比就加 --allow-cross-type",
            deck::type_name_zh(card_type),
            deck::type_name_zh(base_type)
        );
    }
    if base[slot] == idrank {
        return Ok(());
    }
    // 按 card_id 判重：同卡不同突破档换进来也是「同一张卡占两个槽位」，游戏里组不出来
    ensure!(
        !base
            .iter()
            .enumerate()
            .any(|(other, id)| other != slot && id / 10 == idrank / 10),
        "{name} 已占基准卡组其他槽位：换进来会造出重复卡组（同卡不同突破档同样不行）"
    );
    Ok(())
}

/// 候选元信息（解析一次，供守卫、装配与呈现共用）
struct Candidate {
    /// 卡 ID × 10 + 突破档
    idrank: u32,
    /// 卡 ID
    card_id: u32,
    /// 卡名
    name: String,
    /// 卡类型
    card_type: i32,
}

/// 解析候选 idrank（突破档合法 + 卡面可解析）
fn resolve_candidate(idrank: u32) -> Result<Candidate> {
    ensure!(
        idrank % 10 <= deck::FULL_RANK,
        "候选 idrank {idrank} 的突破档非法（需 0..=4）"
    );
    let card = Data::get()?
        .get_card(idrank / 10)
        .with_context(|| format!("候选 idrank {idrank} 无法解析"))?;
    Ok(Candidate {
        idrank,
        card_id: card.card_id,
        name: card.card_name.clone(),
        card_type: card.card_type,
    })
}

/// 跑一轮臂集：首臂为基准卡组，其余逐个按「(槽位, 候选)」变动，返回与 `pairs` 逐位对齐的测量结果
///
/// 一轮内所有臂共用同一段种子序列（第 i 局 `seed + i`），故配对差分只含「换了一张卡」的贡献；
/// 异常局不补采（由 [`arms::run`] 直接报错）。
fn measure_pairs(
    cfg: &Config,
    base: &[u32],
    pairs: &[(usize, u32)],
    runs: u32,
    seed: u64,
    metric: Metric,
    goal: f64,
) -> Result<Vec<Measured>> {
    let mut decks = Vec::with_capacity(pairs.len() + 1);
    decks.push(base.to_vec());
    for (slot, idrank) in pairs {
        let mut deck = base.to_vec();
        deck[*slot] = *idrank;
        decks.push(deck);
    }
    let report = arms::run(arms::Spec { runs, seed }, &decks, goal, |deck, s| {
        deck::measure(cfg, deck, s, metric)
    })?;
    Ok(report.results.iter().map(measured_of).collect())
}

/// 把内核的一臂结果转成测量结果
fn measured_of(result: &arms::ArmResult) -> Measured {
    Measured {
        mean: result.arm.mean(),
        sd: result.arm.sd(),
        paired: result.paired,
        runs_for_goal: result.runs_for_goal,
    }
}

/// 固定基准卡组、只换 `spec.slot` 一个槽位，逐候选同种子配对
pub fn run(cfg: &Config, base: &[u32], spec: &Spec) -> Result<Report> {
    ensure!(spec.runs >= 1, "每臂局数必须 ≥ 1（收到 {}）", spec.runs);
    ensure!(spec.goal > 0.0, "目标分辨率必须 > 0（收到 {}）", spec.goal);
    ensure!(!spec.candidates.is_empty(), "候选列表为空");
    let types = deck::deck_types(base)?;
    let base_type = *types.get(spec.slot).ok_or_else(|| {
        anyhow::anyhow!(
            "变动槽位 {} 越界（基准卡组 {} 张，槽位 0..{}）",
            spec.slot,
            base.len(),
            base.len()
        )
    })?;
    ensure!(
        (0..5).contains(&base_type),
        "变动槽位 {} 上是{}类型卡（类型 {base_type}），不是训练系卡，单卡边际无从谈起",
        spec.slot,
        deck::type_name_zh(base_type)
    );

    // 组装各臂：首臂为基准；候选先过可比性守卫再落臂
    let mut decks = vec![base.to_vec()];
    let mut metas: Vec<(Candidate, bool)> = Vec::with_capacity(spec.candidates.len());
    for idrank in &spec.candidates {
        let cand = resolve_candidate(*idrank)?;
        check_candidate(
            base,
            spec.slot,
            base_type,
            cand.idrank,
            cand.card_type,
            &cand.name,
            spec.allow_cross_type,
        )
        .with_context(|| format!("候选 {idrank} 不可比"))?;
        let mut deck = base.to_vec();
        deck[spec.slot] = cand.idrank;
        let is_reference = deck == base;
        metas.push((cand, is_reference));
        decks.push(deck);
    }
    let started = Instant::now();
    let report = arms::run(
        arms::Spec {
            runs: spec.runs,
            seed: spec.seed,
        },
        &decks,
        spec.goal,
        |deck, seed| deck::measure(cfg, deck, seed, spec.metric),
    )?;

    // 回填测量结果（`arms` 的臂顺序与 decks 一致，跳过首臂），并挂上本族校正
    let family: Vec<Paired> = report.results.iter().map(|result| result.paired).collect();
    let adjusted = spec.correct.adjust(&family);
    let mut rows: Vec<Row> = metas
        .into_iter()
        .zip(report.results.iter())
        .zip(adjusted)
        .map(|(((cand, is_reference), result), adjusted)| Row {
            idrank: cand.idrank,
            card_id: cand.card_id,
            name: cand.name,
            card_type: cand.card_type,
            is_reference,
            measured: measured_of(result),
            adjusted,
        })
        .collect();

    for row in &rows {
        if row.is_reference {
            ensure!(
                row.measured.paired.mean == 0.0 && row.measured.paired.sd == 0.0,
                "自证失败：基准卡自身应给出零差分（实测 Δmean={} Δsd={}）",
                row.measured.paired.mean,
                row.measured.paired.sd
            );
        }
    }
    // 至少一个非基准候选要给出非零差分，否则更可能是卡组根本没换进去
    ensure!(
        rows.iter()
            .any(|row| !row.is_reference && row.measured.paired.mean != 0.0),
        "全部候选都是零差分：候选卡组可能没有真正注入（检查槽位映射）"
    );
    rows.sort_by(|a, b| {
        b.measured
            .paired
            .mean
            .total_cmp(&a.measured.paired.mean)
            .then(a.idrank.cmp(&b.idrank))
    });
    Ok(Report {
        base: base.to_vec(),
        slot: spec.slot,
        base_type,
        spec: spec.clone(),
        rows,
        elapsed: started.elapsed(),
    })
}

/// 逐槽贪婪规格
///
/// 从基准卡组出发，每步测「槽位 × 候选」的全部可放组合，取**正向且越过零**的最优者替换一个槽位，
/// 重复到没有改进或达到步数上限。步与步之间基准卡组不同，故**各步的 Δ 不可跨步比较**
/// （每步的配对都只相对该步的卡组）。
#[derive(Debug, Clone, PartialEq)]
pub struct GreedySpec {
    /// 候选 idrank（显式列表；可含不同突破档）
    pub candidates: Vec<u32>,
    /// 每步每臂局数
    pub runs: u32,
    /// 每步的起始种子（每步都用同一段种子序列）
    pub seed: u64,
    /// 观测轴
    pub metric: Metric,
    /// 反推局数用的目标分辨率
    pub goal: f64,
    /// 允许候选与目标槽位现卡类型不同（默认拒绝：类型变了测的是构成效应）
    pub allow_cross_type: bool,
    /// 最多替换步数（≥ 1）
    pub max_steps: usize,
    /// 多重比较校正口径（族 = 该步测过的全部组合）
    pub correct: Correct,
}

/// 一步里一个「槽位 × 候选」组合的测量结果
#[derive(Debug, Clone, PartialEq)]
pub struct PairRow {
    /// 槽位下标
    pub slot: usize,
    /// 候选 idrank
    pub idrank: u32,
    /// 卡 ID
    pub card_id: u32,
    /// 卡名
    pub name: String,
    /// 卡类型
    pub card_type: i32,
    /// 候选是否就是该槽位现卡（配对增量恒为 0，用作管线自证）
    pub is_reference: bool,
    /// 测量结果（相对**该步**的卡组）
    pub measured: Measured,
    /// 该步组合族内的多重比较校正结果
    pub adjusted: Adjusted,
}

/// 贪婪的一步
#[derive(Debug, Clone, PartialEq)]
pub struct GreedyStep {
    /// 步号（0 基）
    pub index: usize,
    /// 该步开始前的卡组
    pub deck_before: Vec<u32>,
    /// 该步测过的全部组合（按配对增量降序）
    pub rows: Vec<PairRow>,
    /// 采纳的组合（`None` = 该步没有越过零的正向改进，贪婪就此停住）
    pub chosen: Option<PairRow>,
}

/// 贪婪的停止原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// 某一步找不到越过零的正向改进
    NoImprovement,
    /// 用满步数上限（还有改进空间，如需继续请加大 `--greedy-steps`）
    StepsExhausted,
}

impl Stop {
    /// 机读名
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::NoImprovement => "no_improvement",
            Self::StepsExhausted => "steps_exhausted",
        }
    }
}

/// 贪婪报告
#[derive(Debug, Clone)]
pub struct GreedyReport {
    /// 基准卡组（起点）
    pub base: Vec<u32>,
    /// 最终卡组（逐步替换后）
    pub deck: Vec<u32>,
    /// 规格
    pub spec: GreedySpec,
    /// 逐步轨迹（至少一步）
    pub steps: Vec<GreedyStep>,
    /// 停止原因
    pub stop: Stop,
    /// 整批墙钟耗时
    pub elapsed: Duration,
}

impl GreedyReport {
    /// 被替换掉的槽位与卡（按步序）
    #[must_use]
    pub fn replacements(&self) -> Vec<(usize, u32)> {
        self.steps
            .iter()
            .filter_map(|step| step.chosen.as_ref().map(|row| (row.slot, row.idrank)))
            .collect()
    }

    /// 第 `index` 步之后的卡组（该步采纳了替换则为替换后的卡组，否则与步前一致）
    ///
    /// 逐步重放采纳的替换，报告里因此不存冗余卡组副本；`index` 越界即给起点卡组。
    #[must_use]
    pub fn deck_after_step(&self, index: usize) -> Vec<u32> {
        let mut deck = self.base.clone();
        for step in self.steps.iter().take(index + 1) {
            if let Some(best) = &step.chosen {
                deck[best.slot] = best.idrank;
            }
        }
        deck
    }
}

/// 当前卡组下每个候选的可放组合 `(槽位, 候选下标)`
///
/// 默认只认「与槽位现卡同类型」的组合（跨类型测的是构成效应）；`allow` 放开这一条。
/// 候选已占卡组其他槽位（换进来即重复卡组）的组合一律不出现；候选就是该槽位现卡的组合保留
/// （零差分自证臂）。
fn greedy_pairs(deck: &[u32], types: &[i32], candidates: &[Candidate], allow: bool) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    for (ci, cand) in candidates.iter().enumerate() {
        if !allow && !(0..5).contains(&cand.card_type) {
            continue;
        }
        for (slot, _) in deck.iter().enumerate() {
            if !allow && !(0..5).contains(&types[slot]) {
                continue;
            }
            if !allow && types[slot] != cand.card_type {
                continue;
            }
            let same_card_elsewhere = deck
                .iter()
                .enumerate()
                .any(|(other, id)| other != slot && id / 10 == cand.card_id);
            if same_card_elsewhere {
                continue;
            }
            pairs.push((slot, ci));
        }
    }
    pairs
}

/// 逐槽贪婪：每步对全部可放组合做同种子配对，取最优且越过零者替换一个槽位
///
/// 守卫与单槽口径一致且都不静默：基准卡组过短 / 重复 / 无法解析由 [`deck::deck_types`] 报错；
/// 候选重复（同一 idrank 出现两次会重复测量并虚增校正族大小）直接拒绝；组合全空时报错，
/// 全线零差分时报错（候选可能没注进去）。
pub fn run_greedy(cfg: &Config, base: &[u32], spec: &GreedySpec) -> Result<GreedyReport> {
    ensure!(spec.runs >= 1, "每臂局数必须 ≥ 1（收到 {}）", spec.runs);
    ensure!(spec.goal > 0.0, "目标分辨率必须 > 0（收到 {}）", spec.goal);
    ensure!(spec.max_steps >= 1, "步数上限必须 ≥ 1（收到 {}）", spec.max_steps);
    ensure!(!spec.candidates.is_empty(), "候选列表为空");
    for (idx, idrank) in spec.candidates.iter().enumerate() {
        ensure!(
            !spec.candidates[..idx].contains(idrank),
            "候选列表含重复 idrank {idrank}：会重复测量同一组合并虚增校正族大小"
        );
    }
    let mut types = deck::deck_types(base)?;
    let candidates: Vec<Candidate> = spec
        .candidates
        .iter()
        .map(|idrank| resolve_candidate(*idrank))
        .collect::<Result<Vec<_>>>()?;

    let started = Instant::now();
    let mut deck = base.to_vec();
    let mut steps: Vec<GreedyStep> = Vec::with_capacity(spec.max_steps);
    let mut any_nonzero = false;
    let mut stop = Stop::StepsExhausted;
    for index in 0..spec.max_steps {
        let pairs = greedy_pairs(&deck, &types, &candidates, spec.allow_cross_type);
        ensure!(
            !pairs.is_empty(),
            "第 {index} 步没有任何可放的「槽位 × 候选」组合：候选类型与卡组槽位都不匹配？\
             （要测构成效应用 --allow-cross-type）"
        );
        let deck_pairs: Vec<(usize, u32)> = pairs.iter().map(|(slot, ci)| (*slot, candidates[*ci].idrank)).collect();
        let measured = measure_pairs(cfg, &deck, &deck_pairs, spec.runs, spec.seed, spec.metric, spec.goal)?;
        let family: Vec<Paired> = measured.iter().map(|m| m.paired).collect();
        let adjusted = spec.correct.adjust(&family);
        let mut rows: Vec<PairRow> = pairs
            .iter()
            .zip(measured)
            .zip(adjusted)
            .map(|(((slot, ci), measured), adjusted)| {
                let cand = &candidates[*ci];
                PairRow {
                    slot: *slot,
                    idrank: cand.idrank,
                    card_id: cand.card_id,
                    name: cand.name.clone(),
                    card_type: cand.card_type,
                    is_reference: deck[*slot] == cand.idrank,
                    measured,
                    adjusted,
                }
            })
            .collect();
        rows.sort_by(|a, b| {
            b.measured
                .paired
                .mean
                .total_cmp(&a.measured.paired.mean)
                .then(a.slot.cmp(&b.slot))
                .then(a.idrank.cmp(&b.idrank))
        });
        for row in &rows {
            if row.is_reference {
                ensure!(
                    row.measured.paired.mean == 0.0 && row.measured.paired.sd == 0.0,
                    "自证失败：现卡自身应给出零差分（槽位 {} / {}）",
                    row.slot,
                    row.idrank
                );
            }
        }
        any_nonzero |= rows.iter().any(|row| row.measured.paired.mean != 0.0);

        // 采纳：正向（均值 > 0）且校正后越过零的最优组合
        let chosen = rows
            .iter()
            .find(|row| !row.is_reference && row.measured.paired.mean > 0.0 && row.adjusted.beyond)
            .cloned();
        let Some(best) = chosen else {
            steps.push(GreedyStep {
                index,
                deck_before: deck.clone(),
                rows,
                chosen: None,
            });
            stop = Stop::NoImprovement;
            break;
        };
        deck[best.slot] = best.idrank;
        types[best.slot] = best.card_type;
        steps.push(GreedyStep {
            index,
            deck_before: deck.clone(),
            rows,
            chosen: Some(best),
        });
    }

    ensure!(
        any_nonzero,
        "全部组合都是零差分：候选卡组可能没有真正注入（检查槽位映射）"
    );
    Ok(GreedyReport {
        base: base.to_vec(),
        deck,
        spec: spec.clone(),
        steps,
        stop,
        elapsed: started.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：可比性判定——类型、重复与基准自身的三种情形
    #[test]
    fn test_check_candidate() {
        let base = [303_124_u32, 303_174, 303_044, 302_894];
        // 类型不同：默认拒绝，显式放行则通过
        let cross = check_candidate(&base, 0, 0, 302_814, 1, "[耐]稻荷一", false);
        assert!(cross.is_err(), "跨类型默认应拒绝");
        assert!(
            format!("{:#}", cross.expect_err("应报错")).contains("构成效应"),
            "错误要点明测的是构成效应"
        );
        assert!(
            check_candidate(&base, 0, 0, 302_814, 1, "[耐]稻荷一", true).is_ok(),
            "--allow-cross-type 应放行"
        );
        // 候选已占其他槽位 → 重复卡组
        let dup = check_candidate(&base, 0, 0, 303_174, 0, "[速]待兼诗歌剧", false);
        assert!(dup.is_err(), "重复卡组应拒绝");
        assert!(format!("{:#}", dup.expect_err("应报错")).contains("重复卡组"));
        // 同卡不同突破档占着其他槽位（303174 vs 303173）：同样造出重复卡组
        let dup_rank = check_candidate(&base, 0, 0, 303_173, 0, "[速]待兼诗歌剧", false);
        assert!(dup_rank.is_err(), "同卡不同档占其他槽位也应拒绝");
        assert!(format!("{:#}", dup_rank.expect_err("应报错")).contains("重复卡组"));
        // 候选就是基准该槽位的卡 → 放行（自证臂）
        assert!(check_candidate(&base, 0, 0, 303_124, 0, "[速]千明代表", false).is_ok());
        // 正常同类型候选 → 放行
        assert!(check_candidate(&base, 0, 0, 303_114, 0, "[速]乐透心", false).is_ok());
    }

    /// 层1：报告能取到基准卡那一行（自证与展示用）
    #[test]
    fn test_report_reference_lookup() {
        let report = Report {
            base: vec![1, 2, 3, 4, 5],
            slot: 0,
            base_type: 0,
            spec: Spec {
                slot: 0,
                candidates: vec![1],
                runs: 1,
                seed: 0,
                metric: Metric::Score,
                goal: 25.0,
                allow_cross_type: false,
                correct: Correct::None,
            },
            rows: vec![],
            elapsed: std::time::Duration::ZERO,
        };
        assert!(report.reference().is_none(), "空榜没有基准行");
    }

    /// 层1：构造候选元信息（只关心卡 ID 与类型）
    fn cand(idrank: u32, card_type: i32) -> Candidate {
        Candidate {
            idrank,
            card_id: idrank / 10,
            name: format!("#{idrank}"),
            card_type,
        }
    }

    /// 层1：贪婪的可放组合——只认同类型槽位、跳过重复卡组、保留自证臂
    #[test]
    fn test_greedy_pairs() {
        // 卡组：速 / 耐 / 智 / 速 / 智
        let deck = [303_124_u32, 303_044, 302_894, 303_174, 303_064];
        let types = [0, 1, 4, 0, 4];
        let candidates = vec![cand(303_114, 0), cand(302_814, 1), cand(303_064, 4)];
        // 默认口径：每个候选只配同类型槽位；智候选 303064 已占槽 4，
        // 换到槽 2 会造出重复卡组（同卡两槽）故不出现，只剩槽 4 的自证组合
        assert_eq!(
            greedy_pairs(&deck, &types, &candidates, false),
            vec![(0, 0), (3, 0), (1, 1), (4, 2)]
        );

        // 已占其他槽位的候选：只留「就是该槽位现卡」的自证组合（否则换进来即重复卡组）
        let dup = vec![cand(303_174, 0)];
        assert_eq!(greedy_pairs(&deck, &types, &dup, false), vec![(3, 0)]);
        assert_eq!(
            greedy_pairs(&deck, &types, &[cand(303_124, 0)], false),
            vec![(0, 0)],
            "现卡在槽 0：槽 3 换进来即重复卡组，只剩槽 0 的自证组合"
        );

        // 跨类型放开：每个候选对所有槽位都可放（重复卡组除外）
        let cross = greedy_pairs(&deck, &[0, 1, 4, 0, 4], &[cand(303_114, 0)], true);
        assert_eq!(cross, vec![(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
        // 友人类型候选在默认口径下不进训练槽位（会改变构成），放开后才出现
        let friend = [cand(303_054, 5)];
        assert!(greedy_pairs(&deck, &types, &friend, false).is_empty());
        assert_eq!(greedy_pairs(&deck, &types, &friend, true).len(), 5);
    }

    /// 层1：贪婪停止原因的机读名
    #[test]
    fn test_stop_names() {
        assert_eq!(Stop::NoImprovement.name(), "no_improvement");
        assert_eq!(Stop::StepsExhausted.name(), "steps_exhausted");
    }
}
