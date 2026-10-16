//! 通用统计原语
//!
//! 与游戏、与算法无关的纯函数，供搜索、训练与基准共用。约定：
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
}
