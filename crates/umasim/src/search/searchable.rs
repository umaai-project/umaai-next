//! 扁平搜索的剧本适配层
//!
//! `Game` 负责规则推进；本层只补搜索额外需要、且**不能安全给默认值**的能力，
//! 避免把搜索关注点塞进已经很大的 `Game` trait。
//!
//! # 设计约束
//!
//! 除评分口径外一律**不给默认实现**：新增剧本漏实现会编译失败，而不是静默退化——
//! [`FlatSearchGame::fork_for_rollout`] 若被漏掉，每次 rollout 会各摸一次 OS 随机数，
//! 可复现性与 CRN 一起失效，且不会有任何报错。

use anyhow::Result;
use rand::rngs::StdRng;

use crate::game::base::basic::BasicGame;
use crate::game::{Game, Trainer};
use crate::trainer::RandomTrainer;

/// 可被 `FlatSearch`（`search::flat_search`）驱动的剧本
///
/// # 实现须知
///
/// 任何**不由传入 `&mut StdRng` 驱动**的随机流，必须在
/// [`fork_for_rollout`](Self::fork_for_rollout) 中重置，否则搜索层的种子控制不到它。
pub trait FlatSearchGame: Game + Clone + Send + Sync
where
    Self::Action: Send + Sync + Clone,
{
    /// 本剧本 rollout 阶段使用的决策器
    ///
    /// 用关联类型而非 `FlatSearch<G, T>` 的第二个类型参数：每个剧本现阶段只有一种
    /// 基策，多一个参数会把所有调用签名掀开。
    type RolloutTrainer: Trainer<Self> + Send + Sync;

    /// 构造默认 rollout 决策器
    fn default_rollout_trainer() -> Self::RolloutTrainer;

    /// 是否支持 `max_depth > 0` 的截断估值
    ///
    /// 截断后需要 leaf 估值器（属 neural 阶段）。声明为 `false` 的剧本在搜索入口
    /// 被直接拒绝，不会拿到口径错误的标签。
    const SUPPORTS_TRUNCATED_LEAF: bool;

    /// CRN 种子派生用的阶段键（外挂重播种与成本探针使用）
    ///
    /// 默认取当前阶段号（[`Game::stage`]，剧本显式赋号故稳定）；需要更细粒度的剧本可
    /// 覆盖（例如把子状态编进 key）。**号是稳定契约**：改号等于旧种子与基线失效。
    ///
    /// **必须无副作用**：搜索每次都会求值一次。
    fn crn_stage_key(&self) -> u64 {
        self.stage().get()
    }

    /// 按剧本的**真实对局路径**执行 rollout 的根动作
    ///
    /// 必须与 `run_stage` 内执行动作的方式逐字一致：若剧本的阶段推进另走一条随机流，
    /// 而这里走通用 `apply_action`，被评估的那个动作会用上另一条流，排序被系统性
    /// 污染且不报错。
    fn apply_root_action(&mut self, action: &Self::Action, rng: &mut StdRng) -> Result<()>;

    /// 为一次 rollout 建立分支，并初始化剧本内部随机流
    ///
    /// 通用搜索**只能**经由本方法建分支，不得直接 `clone()`——把「克隆」与
    /// 「重置内部随机流」绑成一个不可分割的操作，是防止遗漏的唯一可靠手段。
    fn fork_for_rollout(&self, rollout_seed: u64) -> Self;

    /// 当前局面的搜索评分（单轴排序口径）
    ///
    /// 默认口径：`skill + pt × pt_favor_rate + 五维`，其中 `pt` 已含 PT 折算系数；
    /// `pt_favor_rate = 1.0` 即不偏 PT 的原评分。口径不同的剧本可覆盖本方法。
    fn search_score(&self, pt_favor_rate: f64) -> Result<f64> {
        let parts = self.uma().score_parts()?;
        let pt = (parts.pt as f32 * pt_favor_rate as f32) as i32;
        Ok((parts.skill + pt + parts.five_status.iter().sum::<i32>()) as f64)
    }
}

/// 无剧本基础局的接入（参考实现）
impl FlatSearchGame for BasicGame {
    /// rollout 基策用契约参考实现：基础局不带专用智能算法
    type RolloutTrainer = RandomTrainer;

    /// 基础局没有 leaf 估值器
    const SUPPORTS_TRUNCATED_LEAF: bool = false;

    fn default_rollout_trainer() -> Self::RolloutTrainer {
        RandomTrainer
    }

    /// 基础局无策略流分支，根动作即通用 `apply_action`
    fn apply_root_action(&mut self, action: &Self::Action, rng: &mut StdRng) -> Result<()> {
        self.apply_action(action, rng)
    }

    /// 基础局随机性全部走传入的 `&mut StdRng`，无内部规则流，故只需克隆局面
    fn fork_for_rollout(&self, _rollout_seed: u64) -> Self {
        self.clone()
    }
}
