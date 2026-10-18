//! 通用统计原语
//!
//! 与游戏、与算法无关的纯函数与增量累加器，供搜索、训练与基准共用。约定：
//! 样本不足时取中性值（`0`）或 `None`，一律不 panic。

use serde::{Deserialize, Serialize};

use crate::error::{UmaError, UmaResult};

/// 算术均值；空序列返回 `0`
pub fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// 样本方差（除以 `n − 1`）；样本数 < 2 返回 `0`
pub fn variance(xs: &[f64]) -> f64 {
    let n = xs.len();
    if n < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let ss: f64 = xs.iter().map(|x| (x - m) * (x - m)).sum();
    ss / (n - 1) as f64
}

/// 样本标准差；样本数 < 2 返回 `0`
pub fn stdev(xs: &[f64]) -> f64 {
    variance(xs).sqrt()
}

/// 最小值与最大值；空序列返回 `None`
pub fn min_max(xs: &[f64]) -> Option<(f64, f64)> {
    let mut it = xs.iter().copied();
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), x| (lo.min(x), hi.max(x))))
}

/// 中位数（偶数个样本取中间两数均值）；空序列返回 `None`
pub fn median(xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    let mut sorted = xs.to_vec();
    sorted.sort_by(f64::total_cmp);
    Some(percentile(&sorted, 0.5))
}

/// 升序样本的第 `p` 分位（线性插值，`p ∈ [0, 1]`）；空序列返回 `0`
///
/// 调用方保证 `sorted` 已升序；未排序输入结果无意义。
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = p * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let frac = pos - lo as f64;
        sorted[lo] + (sorted[hi] - sorted[lo]) * frac
    }
}

/// 增量矩统计（在线累加样本数、总和与平方和）
///
/// 与切片式 [`mean`] / [`variance`] / [`stdev`] 的分工：本类型为**流式**场景设计
/// （逐次 rollout 累加、逐维观测），`Copy` 且零堆分配，不保存样本本身。
/// `mean` / `stdev` 口径与切片式一致：样本方差除以 `n − 1`，样本不足取中性值 `0`，
/// 浮点消去导致的极小负方差取 `max(0.0)`。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Moments {
    /// 样本数
    num: u32,
    /// 样本总和
    pub sum: f64,
    /// 样本平方和
    pub sum_sq: f64,
}

impl Moments {
    /// 加入一个样本
    pub fn add(&mut self, value: f64) {
        self.num += 1;
        self.sum += value;
        self.sum_sq += value * value;
    }

    /// 样本数
    pub fn count(&self) -> u32 {
        self.num
    }

    /// 算术平均值；无样本时返回 `0`
    pub fn mean(&self) -> f64 {
        if self.num == 0 {
            return 0.0;
        }
        self.sum / self.num as f64
    }

    /// 样本标准差（贝塞尔校正）；样本数 ≤ 1 时返回 `0`
    ///
    /// 浮点消去可能让方差算出极小负值，故 `max(0.0)` 后再开方（不得为 `NaN`）。
    pub fn stdev(&self) -> f64 {
        if self.num <= 1 {
            return 0.0;
        }
        let n = self.num as f64;
        let variance = (self.sum_sq - self.sum * self.sum / n) / (n - 1.0);
        variance.max(0.0).sqrt()
    }
}

/// 配对差分统计（`b − a`）
///
/// 用于「同一批随机输入下两个臂只差一个变量」的对照口径：配对差分的方差只含两臂的
/// 协方差项 `Var(b) + Var(a) − 2·Cov`，远小于独立抽样——但**只有真正配对（同种子 /
/// 同局号）才有这个性质**；种子不配对时协方差归零，本口径不再成立。
///
/// 区间取**正态近似** `±1.96 × se`（样本很少时偏窄，报告必须写明局数），故不引入
/// 分布表依赖。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paired {
    /// 配对数
    pub n: u32,
    /// 增量均值
    pub mean: f64,
    /// 增量的样本标准差（`n < 2` 时为 `0`）
    pub sd: f64,
    /// 增量均值的标准误（`n < 2` 时为 `0`）
    pub se: f64,
    /// 配对统计量（`se = 0` 时为 `0`）
    pub t: f64,
    /// 95% 置信半径（正态近似）
    pub ci95: f64,
    /// 增量为正的配对数
    pub wins: u32,
    /// 增量为负的配对数
    pub losses: u32,
    /// 增量恰为 `0` 的配对数
    pub ties: u32,
}

impl Paired {
    /// 增量均值是否落在零之外（正态近似）
    ///
    /// 只作**单对比较**的粗判据：多臂比较会踩多重比较，报告不得据此直接下「显著」结论。
    #[must_use]
    pub fn beyond_zero(&self) -> bool {
        self.n >= 2 && self.mean.abs() > self.ci95
    }
}

/// 配对差分统计：`a` 为对照臂、`b` 为变动臂，逐位相减
///
/// 两序列必须等长且非空；长度不等即调用方 bug，报错而不静默截断。
pub fn paired(a: &[f64], b: &[f64]) -> UmaResult<Paired> {
    if a.len() != b.len() {
        return Err(UmaError::InvalidInput(format!(
            "配对样本长度不一致: {} vs {}",
            a.len(),
            b.len()
        )));
    }
    if a.is_empty() {
        return Err(UmaError::InvalidInput("配对样本为空".to_string()));
    }
    let diff: Vec<f64> = a.iter().zip(b.iter()).map(|(x, y)| y - x).collect();
    let n = diff.len();
    let mean = mean(&diff);
    let sd = stdev(&diff);
    let se = if n < 2 { 0.0 } else { sd / (n as f64).sqrt() };
    Ok(Paired {
        n: n as u32,
        mean,
        sd,
        se,
        t: if se > 0.0 { mean / se } else { 0.0 },
        ci95: 1.96 * se,
        wins: diff.iter().filter(|d| **d > 0.0).count() as u32,
        losses: diff.iter().filter(|d| **d < 0.0).count() as u32,
        ties: diff.iter().filter(|d| **d == 0.0).count() as u32,
    })
}

/// 分辨 `effect` 大小的配对效应所需的配对数
///
/// 依据：80% 功效、α = 0.05 双侧的正态近似，`n ≈ (2.8 × sd / effect)²`，向上取整。
/// `sd` 必须是**配对差分**的样本标准差（不是单臂标准差）；`sd` 或 `effect` 非正、
/// 非有限时返回 `0`（中性值：无法给出建议）。
#[must_use]
pub fn paired_runs_for_effect(sd: f64, effect: f64) -> u32 {
    if !sd.is_finite() || !effect.is_finite() || sd <= 0.0 || effect <= 0.0 {
        return 0;
    }
    let n = (2.8 * sd / effect).powi(2).ceil();
    if n >= f64::from(u32::MAX) { u32::MAX } else { n as u32 }
}

/// 标准正态分布函数 `Φ(z)`
///
/// 用 Abramowitz–Stegun 7.1.26 的 `erf` 近似（绝对误差 < 1.5e-7），够撑 p 值与判定；
/// 引入它只是为了做多重比较校正，**不引分布表依赖**。`NaN` 原样传出。
#[must_use]
pub fn normal_cdf(z: f64) -> f64 {
    if z.is_nan() {
        return f64::NAN;
    }
    // 近似式在中点有 ~1e-9 的偏置，专门钉住 0：p 值公式在 t = 0 处必须精确是 1
    if z == 0.0 {
        return 0.5;
    }
    let x = z / std::f64::consts::SQRT_2;
    let ax = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * ax);
    let poly =
        t * (0.254_829_592 + t * (-0.284_496_736 + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-ax * ax).exp();
    if x < 0.0 { (1.0 - erf) / 2.0 } else { (1.0 + erf) / 2.0 }
}

/// 标准正态分位函数 `Φ⁻¹(p)`（Acklam 有理逼近，相对误差 ~1e-9）
///
/// `p ≤ 0` 给 `-∞`、`p ≥ 1` 给 `+∞`（区间端点的正确极限），`NaN` 原样传出。
#[must_use]
pub fn normal_quantile(p: f64) -> f64 {
    if p.is_nan() {
        return f64::NAN;
    }
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_690e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];
    /// 低 / 高尾的中央区界（Acklam 原文取值）
    const P_LOW: f64 = 0.024_25;
    if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= 1.0 - P_LOW {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
}

/// 双侧 p 值（正态近似）：`p = 2 × (1 − Φ(|t|))`
///
/// 与 [`Paired`] 的 CI 用同一套正态近似；`t` 非有限时返回 `1`（无法判定）。
#[must_use]
pub fn two_sided_p(t: f64) -> f64 {
    if !t.is_finite() {
        return 1.0;
    }
    (2.0 * (1.0 - normal_cdf(t.abs()))).clamp(0.0, 1.0)
}

/// 多重比较校正口径
///
/// 一族 = 同一次运行里互相比过的一组候选（多臂比较会踩多重比较，故默认不校正、只报 CI）。
/// 两档都走**正态近似**，与 [`Paired`] 的区间口径一致，不引分布表依赖。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Correct {
    /// 不校正（默认：只报 CI，不判「显著」）
    None,
    /// Bonferroni：族内每个比较的 α 除以族大小 `m`
    Bonferroni,
    /// Holm step-down：按 p 升序，第 k 小的比较用 `α / (m − k + 1)`
    Holm,
}

impl Correct {
    /// 默认显著水平（双侧）
    pub const DEFAULT_ALPHA: f64 = 0.05;

    /// 解析口径名（`none` / `bonferroni` / `holm`）；不认识返回 `None`
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "none" => Some(Self::None),
            "bonferroni" => Some(Self::Bonferroni),
            "holm" => Some(Self::Holm),
            _ => None,
        }
    }

    /// 机读名
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bonferroni => "bonferroni",
            Self::Holm => "holm",
        }
    }

    /// 对一族的配对比较做校正（α 取 [`Correct::DEFAULT_ALPHA`]）
    ///
    /// 返回表与入参**逐位对齐**（同一族内的顺序无关紧要；Holm 内部按 p 升序排序再还原）。
    #[must_use]
    pub fn adjust(&self, rows: &[Paired]) -> Vec<Adjusted> {
        self.adjust_with(rows, Self::DEFAULT_ALPHA)
    }

    /// 同上，显著水平可注入
    ///
    /// `α` 非正或非有限时报错级的退化输入按 `0.05` 兜底，避免算出 `∞` 置信半径。
    #[must_use]
    pub fn adjust_with(&self, rows: &[Paired], alpha: f64) -> Vec<Adjusted> {
        let alpha = if alpha.is_finite() && alpha > 0.0 && alpha < 1.0 {
            alpha
        } else {
            Self::DEFAULT_ALPHA
        };
        let m = rows.len().max(1);
        let ps: Vec<f64> = rows.iter().map(|row| two_sided_p(row.t)).collect();

        // Holm：先按 p 升序定「逐步档位」，再把校正 p 还原到入参顺序
        let mut order: Vec<usize> = (0..rows.len()).collect();
        order.sort_by(|a, b| ps[*a].total_cmp(&ps[*b]).then(a.cmp(b)));
        let mut level: Vec<f64> = vec![alpha; rows.len()];
        let mut p_adj_sorted: Vec<f64> = Vec::with_capacity(rows.len());
        let mut running = 0.0_f64;
        for (rank, idx) in order.iter().enumerate() {
            let denom = (m - rank) as f64;
            level[*idx] = match self {
                Self::None => alpha,
                Self::Bonferroni => alpha / m as f64,
                Self::Holm => alpha / denom,
            };
            let factor = match self {
                Self::None => 1.0,
                Self::Bonferroni => m as f64,
                Self::Holm => denom,
            };
            running = running.max((ps[*idx] * factor).min(1.0));
            p_adj_sorted.push(running);
        }
        let mut p_adj = vec![1.0_f64; rows.len()];
        for (rank, idx) in order.iter().enumerate() {
            p_adj[*idx] = p_adj_sorted[rank];
        }

        rows.iter()
            .enumerate()
            .map(|(i, row)| {
                let z = normal_quantile(1.0 - level[i] / 2.0);
                Adjusted {
                    mean: row.mean,
                    p: ps[i],
                    p_adj: p_adj[i],
                    z,
                    ci: if z.is_finite() { z * row.se } else { f64::INFINITY },
                    beyond: p_adj[i] <= alpha,
                }
            })
            .collect()
    }
}

/// 单行配对比较的校正结果
///
/// `beyond` 一律由**校正后**的 `p` 判定（`p_adj ≤ α`）；`ci` 用该行档位的分位数换算，
/// Holm 下每行的档位不同（`α / (m − k + 1)`），判定则以逐步规则的最终 p 为准。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adjusted {
    /// 配对增量均值（原样带出，便于一行读完）
    pub mean: f64,
    /// 双侧 p 值（正态近似）
    pub p: f64,
    /// 校正后 p 值（`None` 档等于 `p`）
    pub p_adj: f64,
    /// 该行档位对应的置信倍数
    pub z: f64,
    /// 校正后置信半径（`z × se`）
    pub ci: f64,
    /// 校正后是否越过零
    pub beyond: bool,
}

impl Adjusted {
    /// 中性值（`p = 1`、置信半径 0、不越过零）
    ///
    /// 供「先建行、后挂校正」的流程当占位用：落成报告前必然被 [`Correct::adjust`] 的结果覆盖，
    /// 不会作为结论露面。
    #[must_use]
    pub const fn neutral() -> Self {
        Self {
            mean: 0.0,
            p: 1.0,
            p_adj: 1.0,
            z: 0.0,
            ci: 0.0,
            beyond: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：均值与方差/标准差对手算样本成立
    #[test]
    fn test_mean_variance_stdev() {
        let xs = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        assert!((mean(&xs) - 5.0).abs() < 1e-12);
        // 样本方差（n−1）= 32/7
        assert!((variance(&xs) - 32.0 / 7.0).abs() < 1e-12);
        assert!((stdev(&xs) - (32.0f64 / 7.0).sqrt()).abs() < 1e-12);
    }

    /// 层1：空序列与单样本走中性值，不 panic
    #[test]
    fn test_degenerate() {
        assert_eq!(mean(&[]), 0.0);
        assert_eq!(variance(&[]), 0.0);
        assert_eq!(variance(&[3.0]), 0.0);
        assert_eq!(stdev(&[3.0]), 0.0);
        assert_eq!(min_max(&[]), None);
        assert_eq!(median(&[]), None);
        assert_eq!(percentile(&[], 0.5), 0.0);
    }

    /// 层1：中位数奇偶两种情形
    #[test]
    fn test_median() {
        assert_eq!(median(&[3.0]), Some(3.0));
        assert_eq!(median(&[1.0, 3.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
    }

    /// 层1：分位线性插值与原实现口径一致
    #[test]
    fn test_percentile() {
        let sorted = [10.0, 20.0, 30.0, 40.0];
        assert_eq!(percentile(&sorted, 0.0), 10.0);
        assert_eq!(percentile(&sorted, 1.0), 40.0);
        assert_eq!(percentile(&sorted, 0.5), 25.0);
        assert!((percentile(&sorted, 0.2) - 16.0).abs() < 1e-12);
    }

    /// 层1：min/max 正确
    #[test]
    fn test_min_max() {
        assert_eq!(min_max(&[3.0]), Some((3.0, 3.0)));
        assert_eq!(min_max(&[-1.0, 5.0, 2.0]), Some((-1.0, 5.0)));
    }

    /// 层1：增量矩统计与切片式口径一致（均值 / 样本标准差 / 退化样本）
    #[test]
    fn test_moments_matches_slice_stats() {
        let xs = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let mut m = Moments::default();
        for &x in &xs {
            m.add(x);
        }
        assert_eq!(m.count(), xs.len() as u32);
        assert_eq!(m.sum, xs.iter().sum::<f64>());
        // 单趟（平方和）与两趟（离差平方和）存在末位差异，故用容差比对
        assert!((m.mean() - mean(&xs)).abs() < 1e-12);
        assert!((m.stdev() - stdev(&xs)).abs() < 1e-12);
    }

    /// 层1：增量矩统计的退化样本取中性值，且全等样本不得出 NaN
    #[test]
    fn test_moments_degenerate() {
        let empty = Moments::default();
        assert_eq!(empty.count(), 0);
        assert_eq!(empty.mean(), 0.0);
        assert_eq!(empty.stdev(), 0.0);

        let mut one = Moments::default();
        one.add(7.0);
        assert_eq!(one.count(), 1);
        assert_eq!(one.mean(), 7.0);
        assert_eq!(one.stdev(), 0.0);

        // 全等样本：浮点消去可能给出负方差，必须被 max(0.0) 吃掉
        let mut same = Moments::default();
        for _ in 0..64 {
            same.add(56_712.0);
        }
        assert_eq!(same.mean(), 56_712.0);
        assert_eq!(same.stdev(), 0.0);
        assert!(same.stdev().is_finite());
    }

    /// 层1：配对差分与手算一致（含胜 / 负 / 平计数与 t、置信半径）
    #[test]
    fn test_paired_hand_computed() {
        let a = [10.0, 20.0, 30.0, 40.0];
        let b = [12.0, 18.0, 30.0, 44.0];
        let p = paired(&a, &b).expect("等长非空应可配对");
        assert_eq!(p.n, 4);
        assert!((p.mean - 1.0).abs() < 1e-12, "增量均值 = (+2−2+0+4)/4");
        // 离差平方和 1+9+1+9 = 20，样本方差 20/3
        assert!((p.sd - (20.0f64 / 3.0).sqrt()).abs() < 1e-12);
        assert!((p.se - p.sd / 2.0).abs() < 1e-12);
        assert!((p.t - p.mean / p.se).abs() < 1e-12);
        assert!((p.ci95 - 1.96 * p.se).abs() < 1e-12);
        assert_eq!((p.wins, p.losses, p.ties), (2, 1, 1));
        assert!(!p.beyond_zero(), "均值 1.0 小于置信半径，不应判为越过零");
    }

    /// 层1：配对方向可逆；单样本与全等样本取中性值且不 panic
    #[test]
    fn test_paired_degenerate_and_symmetry() {
        let a = [5.0, 7.0];
        let b = [6.0, 9.0];
        let fwd = paired(&a, &b).expect("应可配对");
        let rev = paired(&b, &a).expect("应可配对");
        assert!((fwd.mean + rev.mean).abs() < 1e-12, "方向反转应取负");
        assert_eq!((fwd.wins, fwd.losses), (rev.losses, rev.wins));

        let one = paired(&[1.0], &[3.0]).expect("单样本应可配对");
        assert_eq!(one.n, 1);
        assert!((one.mean - 2.0).abs() < 1e-12);
        assert_eq!(one.sd, 0.0);
        assert_eq!(one.se, 0.0);
        assert_eq!(one.t, 0.0);
        assert_eq!(one.ci95, 0.0);
        assert!(!one.beyond_zero(), "单样本不足以判定越过零");

        let same = paired(&[4.0; 8], &[4.0; 8]).expect("应可配对");
        assert_eq!(same.ties, 8);
        assert_eq!(same.mean, 0.0);
        assert!(!same.beyond_zero());
    }

    /// 层1：长度不一致与空样本报错，不静默截断
    #[test]
    fn test_paired_rejects_bad_input() {
        assert!(paired(&[1.0, 2.0], &[1.0]).is_err(), "长度不一致应报错");
        assert!(paired(&[], &[]).is_err(), "空样本应报错");
    }

    /// 层1：所需配对数按 80% 功效公式取值，退化输入取中性值
    #[test]
    fn test_paired_runs_for_effect() {
        // Δsd = 250 时分辨 25 分 → (2.8 × 10)² = 784
        assert_eq!(paired_runs_for_effect(250.0, 25.0), 784);
        // 效应越小所需局数越多（单调性），效应翻倍 → 局数约四分之一
        let coarse = paired_runs_for_effect(250.0, 100.0);
        assert!(coarse < 784, "效应变大所需局数应下降");
        assert_eq!(coarse, 49, "(2.8 × 2.5)² = 49");
        // 退化输入：零 / 负 / 非有限一律中性值
        assert_eq!(paired_runs_for_effect(0.0, 100.0), 0);
        assert_eq!(paired_runs_for_effect(250.0, 0.0), 0);
        assert_eq!(paired_runs_for_effect(f64::NAN, 100.0), 0);
        assert_eq!(paired_runs_for_effect(250.0, f64::INFINITY), 0);
        assert_eq!(paired_runs_for_effect(f64::MAX, 1e-300), u32::MAX);
    }

    /// 层1：正态分布函数与分位数对手算点成立，且互为逆运算
    #[test]
    fn test_normal_cdf_and_quantile() {
        assert_eq!(normal_cdf(0.0), 0.5, "中点钉死，p 值公式在 t = 0 处才能精确为 1");
        // 1.96 σ 的双侧 5% 界：Φ(1.959964) = 0.975
        assert!((normal_cdf(1.959_964) - 0.975).abs() < 1e-6);
        assert!((normal_cdf(-1.959_964) - 0.025).abs() < 1e-6);
        assert!((normal_cdf(1.0) - 0.841_344_7).abs() < 1e-6);
        assert!((normal_cdf(0.0) - (1.0 - normal_cdf(0.0))).abs() < 1e-12, "关于 0 对称");

        assert!(normal_quantile(0.5).abs() < 1e-9);
        assert!((normal_quantile(0.975) - 1.959_964).abs() < 1e-6);
        assert!((normal_quantile(0.025) + 1.959_964).abs() < 1e-6);
        for p in [0.001, 0.025, 0.2, 0.5, 0.8, 0.975, 0.999] {
            let round = normal_cdf(normal_quantile(p));
            assert!((round - p).abs() < 1e-6, "Φ(Φ⁻¹({p})) = {round}");
        }

        // 退化输入：端点给无穷（正确的极限）、NaN 原样传出
        assert_eq!(normal_quantile(0.0), f64::NEG_INFINITY);
        assert_eq!(normal_quantile(1.0), f64::INFINITY);
        assert!(normal_quantile(f64::NAN).is_nan());
        assert!(normal_cdf(f64::NAN).is_nan());
        assert_eq!(normal_cdf(f64::INFINITY), 1.0);
        assert_eq!(normal_cdf(f64::NEG_INFINITY), 0.0);
    }

    /// 层1：双侧 p 值与分位数一致（|t| = 1.959964 → p = 0.05）
    #[test]
    fn test_two_sided_p() {
        assert!((two_sided_p(0.0) - 1.0).abs() < 1e-12);
        assert!((two_sided_p(1.959_964) - 0.05).abs() < 1e-6);
        assert!((two_sided_p(-1.959_964) - 0.05).abs() < 1e-6, "只取 |t|");
        assert!((two_sided_p(3.0) - 0.002_699_8).abs() < 1e-6, "3σ 双侧 ≈ 0.0027");
        assert_eq!(two_sided_p(f64::NAN), 1.0, "无法判定取中性值");
        assert_eq!(two_sided_p(f64::INFINITY), 1.0);
    }

    /// 层1：构造一行配对结果（只关心 `t` 与 `se` 的校正输入）
    fn paired_of(t: f64, se: f64) -> Paired {
        Paired {
            n: 100,
            mean: t * se,
            sd: se * 10.0,
            se,
            t,
            ci95: 1.96 * se,
            wins: 0,
            losses: 0,
            ties: 0,
        }
    }

    /// 层1：不校正档与单对判据一致，且与入参逐位对齐
    #[test]
    fn test_correct_none() {
        assert_eq!(Correct::parse("none"), Some(Correct::None));
        assert_eq!(Correct::parse("bonferroni"), Some(Correct::Bonferroni));
        assert_eq!(Correct::parse("holm"), Some(Correct::Holm));
        assert_eq!(Correct::parse("nope"), None);
        assert_eq!(Correct::None.name(), "none");

        let rows = [paired_of(3.0, 1.0), paired_of(1.0, 1.0)];
        let adjusted = Correct::None.adjust(&rows);
        assert_eq!(adjusted.len(), 2);
        assert!((adjusted[0].p - two_sided_p(3.0)).abs() < 1e-12, "不校正时 p_adj = p");
        assert!((adjusted[0].p_adj - adjusted[0].p).abs() < 1e-12);
        assert!(adjusted[0].beyond, "3σ 在 α=0.05 下越过零");
        assert!(!adjusted[1].beyond, "1σ 不足以越过零");
        assert!((adjusted[0].ci - 1.96 * 1.0).abs() < 1e-4, "置信半径回到 1.96σ");
        assert!((adjusted[0].mean - 3.0).abs() < 1e-12, "均值原样带出，便于一行读完");
        assert!(Correct::None.adjust(&[]).is_empty(), "空族不 panic");
    }

    /// 层1：Bonferroni 把 α 除以族大小（同一 p 在更大的族里可能不再越过零）
    #[test]
    fn test_correct_bonferroni() {
        let row = paired_of(3.0, 1.0);
        let small = Correct::Bonferroni.adjust(&[row]);
        assert!((small[0].p_adj - row_p(&row) * 1.0).abs() < 1e-12, "m = 1 时不放大");
        assert!(small[0].beyond);

        let rows = vec![row; 50];
        let wide = Correct::Bonferroni.adjust(&rows);
        assert!(wide[0].p_adj > 0.05, "p ≈ 0.0027 × 50 > 0.05");
        assert!(!wide[0].beyond, "校正后不再越过零");
        assert!(wide[0].ci > small[0].ci, "校正后的置信半径更宽");

        // 显著水平可注入：α = 0.001 时 3σ 也不再越过零
        let strict = Correct::Bonferroni.adjust_with(&[row], 0.001);
        assert!(!strict[0].beyond);
        // 非法 α 退回默认档
        let fallback = Correct::None.adjust_with(&[row], 0.0);
        assert!(fallback[0].beyond, "非法 α 退回 0.05");
    }

    /// 层1：Holm step-down 的校正 p 单调、逐步档位按 p 升序生效
    #[test]
    fn test_correct_holm() {
        // p 升序：3σ（≈0.0027）、2σ（≈0.0455）、0σ（1.0）
        let rows = [paired_of(3.0, 1.0), paired_of(2.0, 1.0), paired_of(0.0, 1.0)];
        let adjusted = Correct::Holm.adjust(&rows);
        assert_eq!(adjusted.len(), 3);
        // 最小 p 的档位是 α/m：0.0027 × 3 = 0.0081 < 0.05 → 越过零
        assert!((adjusted[0].p_adj - row_p(&rows[0]) * 3.0).abs() < 1e-12);
        assert!(adjusted[0].beyond);
        // 第二小：0.0455 × 2 = 0.091 > 0.05，且校正 p 单调不减
        assert!((adjusted[1].p_adj - row_p(&rows[1]) * 2.0).abs() < 1e-12);
        assert!(adjusted[1].p_adj >= adjusted[0].p_adj, "校正 p 单调不减");
        assert!(!adjusted[1].beyond);
        assert!(!adjusted[2].beyond);
        assert_eq!(adjusted[2].p_adj, 1.0, "最大 p 的校正值封顶 1");
        // 档位随 p 升序变松：最小 p 的置信半径反而比不校正档更宽
        assert!(adjusted[0].ci > 1.96);
    }

    /// 层1：构造行对应的双侧 p（p 值本身另有专门用例，这里只喂给校正逻辑）
    fn row_p(row: &Paired) -> f64 {
        two_sided_p(row.t)
    }
}
