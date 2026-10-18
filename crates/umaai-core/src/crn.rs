//! CRN（公共随机数）rollout 种子表
//!
//! 一次搜索内的所有候选共享同一张表：候选 i 的第 j 次 rollout 一律取
//! [`RolloutSeeds::seed_at`]`(j)`，使分数差异尽量只来自动作本身而非随机流不同。
//!
//! # 为什么候选索引不参与派生
//!
//! CRN 的方差削减来自配对样本的协方差项 `Var(X_a − X_b) = Var(X_a) + Var(X_b) − 2·Cov`；
//! 若种子按 `hash(root, 候选, rollout)` 派生，候选间协方差归零，比较方差退回独立抽样——
//! 那不是 CRN，只是可复现的独立抽样。
//!
//! 派生常数与 [`crate::rng::GAMMA`] 同源，冻结不可改。

use rand::Rng;

use crate::rng::{GAMMA, splitmix64};

/// 一次搜索内所有候选共享的 rollout 种子表
///
/// 按需计算而非预分配数组：UCB 分配下 rollout 序号的上界不确定
/// （`search_n` 会被分组大小打超），预分配容易算错长度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RolloutSeeds {
    /// 本次搜索的根种子
    root: u64,
}

impl RolloutSeeds {
    /// 从搜索入口 RNG 抽取根种子
    ///
    /// 只抽一次，使外层（整局种子）能罩住整次搜索。
    pub fn from_rng(rng: &mut impl Rng) -> Self {
        Self { root: rng.next_u64() }
    }

    /// 用指定根种子构造（测试与回归基准用）
    pub fn from_root(root: u64) -> Self {
        Self { root }
    }

    /// 本次搜索的根种子
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 第 `rollout` 次 rollout 的种子（SplitMix64 派生）
    ///
    /// **不吃候选索引**，理由见模块文档：同一 `rollout` 序号在所有候选上返回同一值。
    pub fn seed_at(&self, rollout: usize) -> u64 {
        splitmix64(
            self.root
                .wrapping_add((rollout as u64).wrapping_add(1).wrapping_mul(GAMMA)),
        )
    }
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;

    /// 层1：同根种子同序号必须确定（可复现性的最底层保证）
    #[test]
    fn test_seed_at_deterministic() {
        let a = RolloutSeeds::from_root(42);
        let b = RolloutSeeds::from_root(42);
        for j in [0usize, 1, 7, 255, 1023, 12287] {
            assert_eq!(a.seed_at(j), b.seed_at(j), "同根种子同序号必须一致");
        }
    }

    /// 层1：派生口径冻结——可复现性的回归锚点
    ///
    /// 若改动 `GAMMA` / finalizer / 派生式，本用例必须红。
    #[test]
    fn test_seed_at_frozen_vector() {
        let seeds = RolloutSeeds::from_root(42);
        let cases = [
            (0usize, 0xbdd7_3226_2feb_6e95u64),
            (1, 0x28ef_e333_b266_f103),
            (7, 0xccf6_35ee_9e9e_2fa4),
            (255, 0x6acc_e368_974e_61ee),
            (1023, 0xad1f_80ae_1003_9789),
            (12287, 0x8207_98c8_7880_21ba),
        ];
        for (j, expect) in cases {
            assert_eq!(seeds.seed_at(j), expect, "rollout {j} 派生值偏离冻结口径");
        }
        assert_eq!(RolloutSeeds::from_root(43).seed_at(0), 0xba69_ec90_eb4f_ef88);
    }

    /// 层1：不同 rollout 序号必须给出不同种子（否则所有 rollout 退化成同一局）
    #[test]
    fn test_seed_at_distinct_per_rollout() {
        let seeds = RolloutSeeds::from_root(42);
        let got: Vec<u64> = (0..1024).map(|j| seeds.seed_at(j)).collect();
        let mut uniq = got.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), got.len(), "同一根种子下各 rollout 序号不得碰撞");
    }

    /// 层1：不同根种子给出不同序列（否则换 seed 跑批等于没换）
    #[test]
    fn test_distinct_root_distinct_sequence() {
        let a = RolloutSeeds::from_root(42);
        let b = RolloutSeeds::from_root(43);
        let same = (0..256).filter(|&j| a.seed_at(j) == b.seed_at(j)).count();
        assert_eq!(same, 0, "不同根种子不应产生相同序列");
    }

    /// 层1：`from_rng` 由入口 RNG 决定，入口种子固定则根种子固定
    #[test]
    fn test_from_rng_follows_entry_seed() {
        let mut rng1 = StdRng::seed_from_u64(7);
        let mut rng2 = StdRng::seed_from_u64(7);
        let s1 = RolloutSeeds::from_rng(&mut rng1);
        let s2 = RolloutSeeds::from_rng(&mut rng2);
        assert_eq!(s1.root(), s2.root(), "入口种子相同则根种子相同");
    }
}
