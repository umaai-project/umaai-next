//! 通用统计原语
//!
//! 与游戏、与算法无关的纯函数与增量累加器，供搜索、训练与基准共用。约定：
//! 样本不足时取中性值（`0`）或 `None`，一律不 panic。

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
}
