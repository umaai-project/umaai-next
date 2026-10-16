//! CRN（公共随机数）rollout 种子表
//!
//! 一次搜索内的所有候选共享同一张表：候选 i 的第 j 次 rollout 一律取
//! [`RolloutSeeds::seed_at`]`(j)`，使分数差异尽量只来自动作本身而非随机流不同。
//!
//! # 为什么候选索引不参与派生
//!
//! CRN 的方差削减来自配对样本的协方差项 `Var(X_a − X_b) = Var(X_a) + Var(X_b) − 2·Cov`；
//! 若种子按 `hash(root, 候选, rollout)` 派生，候选间协方差归零，比较方差退回独立抽样——
//! 那不是 CRN，只是可复现的独立抽样。[`RolloutSeeds::stage_seed`] 同理只吃 `(种子, 回合, 阶段)`。
//!
//! 派生常数与 [`crate::rng::GAMMA`] 同源，冻结不可改。

use rand::Rng;

use crate::rng::{GAMMA, MIX_A, splitmix64};

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

    /// 由某个 rollout 种子再派生「该 rollout 在指定 `(回合, 阶段键)` 上的随机流种子」
    ///
    /// 供**规则层未接管随机流**的剧本使用（外挂 CRN）：每进入一个阶段按本方法重播种，
    /// 各候选在同一 `(回合, 阶段)` 上抽到同一份随机性。规则层已按 `(rollout 种子, 回合)`
    /// 自行派生的剧本不需要它。
    ///
    /// 不吃候选索引，理由同 [`Self::seed_at`]。回合可能为负（未初始化局面），
    /// 转 `u64` 前先做无符号重解释，避免符号扩展碰撞。
    pub fn stage_seed(rollout_seed: u64, turn: i32, stage: u64) -> u64 {
        let turn_bits = (turn as i64) as u64;
        let mixed = rollout_seed
            .wrapping_add(turn_bits.wrapping_add(1).wrapping_mul(GAMMA))
            .wrapping_add(stage.wrapping_add(1).wrapping_mul(MIX_A));
        splitmix64(mixed)
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

    /// 层1：`stage_seed` 派生口径冻结——可复现性的回归锚点
    ///
    /// 若改动常数、加减次序或 finalizer，本用例必须红。
    #[test]
    fn test_stage_seed_frozen_vector() {
        let base = RolloutSeeds::from_root(42).seed_at(3);
        assert_eq!(base, 0x581c_e1ff_0e4a_e394, "前置：seed_at(3) 派生值偏离冻结口径");
        let cases = [
            ((0i32, 0u64), 0xb9a8_61b1_7635_e60a_u64),
            ((12, 3), 0xba37_bb29_b0b4_ef28),
            ((77, 4), 0x646d_8f61_6f96_1a54),
            ((-1, 0), 0xa7d8_388d_1fe1_0d73),
            ((0, 5), 0x3aa5_8fba_80a5_a080),
        ];
        for ((turn, stage), expect) in cases {
            assert_eq!(
                RolloutSeeds::stage_seed(base, turn, stage),
                expect,
                "stage_seed({turn}, {stage}) 派生值偏离冻结口径"
            );
        }
    }

    /// 层1：`stage_seed` 确定，且 `(回合, 阶段)` 任一不同即给出不同种子
    #[test]
    fn test_stage_seed_distinct_per_turn_and_stage() {
        let base = RolloutSeeds::from_root(42).seed_at(3);
        assert_eq!(
            RolloutSeeds::stage_seed(base, 12, 3),
            RolloutSeeds::stage_seed(base, 12, 3),
            "同参数必须给出同种子"
        );
        let mut seen = Vec::new();
        for turn in 0..78i32 {
            for stage in 0..5u64 {
                seen.push(RolloutSeeds::stage_seed(base, turn, stage));
            }
        }
        let total = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), total, "(回合, 阶段) 组合不得碰撞");
    }

    /// 层1：不同 rollout 的同一 `(回合, 阶段)` 必须是不同随机流
    ///
    /// 否则所有 rollout 在该阶段抽到完全一样的结果，方差直接塌掉。
    #[test]
    fn test_stage_seed_distinct_per_rollout() {
        let seeds = RolloutSeeds::from_root(42);
        let got: Vec<u64> = (0..512)
            .map(|j| RolloutSeeds::stage_seed(seeds.seed_at(j), 20, 3))
            .collect();
        let mut uniq = got.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), got.len(), "不同 rollout 在同一阶段不得共用随机流");
    }
}
